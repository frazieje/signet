use clap::Parser;
use envoy_types::pb::envoy::config::core::v3::{
    HeaderMap as EnvoyHeaderMap, HeaderValue as EnvoyHeaderValue,
};
use envoy_types::pb::envoy::service::ext_proc::v3::{
    HttpBody, HttpHeaders, ProcessingRequest, processing_request, processing_response,
};
use envoy_types::pb::envoy::service::ext_proc::v3::external_processor_client::ExternalProcessorClient;
use rand::Rng;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Semaphore, mpsc};
use tokio_stream::wrappers::ReceiverStream;

#[derive(Parser)]
#[command(name = "signet-loadtest", about = "Load test tool for Signet")]
struct Args {
    /// Signet gRPC address
    #[arg(long, default_value = "http://localhost:50051")]
    addr: String,

    /// Concurrent streams
    #[arg(short, long, default_value_t = 10)]
    concurrency: usize,

    /// Total requests
    #[arg(short = 'n', long, default_value_t = 1000)]
    requests: usize,

    /// Minimum body size in bytes
    #[arg(long, default_value_t = 64)]
    min_body: usize,

    /// Maximum body size in bytes
    #[arg(long, default_value_t = 65536)]
    max_body: usize,

    /// Sweep concurrency: escalate until P50 exceeds threshold (in ms)
    #[arg(long)]
    sweep: Option<f64>,

    /// Sweep body size: test each band from 256B to 5MB at fixed concurrency
    #[arg(long, default_value_t = false)]
    sweep_body: bool,
}

struct RequestResult {
    duration: Duration,
    signed: bool,
}

struct RunStats {
    total: usize,
    signed: usize,
    errors: usize,
    p50: Duration,
    p90: Duration,
    p99: Duration,
    max: Duration,
    throughput: f64,
    wall_time: Duration,
}

async fn run_one_request(mut client: ExternalProcessorClient<tonic::transport::Channel>, min_body: usize, max_body: usize) -> Result<RequestResult, String> {
    let start = Instant::now();

    let (tx, rx) = mpsc::channel::<ProcessingRequest>(4);

    let stream = ReceiverStream::new(rx);
    let mut response_stream = client
        .process(stream)
        .await
        .map_err(|e| format!("process: {e}"))?
        .into_inner();

    // Send ResponseHeaders
    let headers_msg = ProcessingRequest {
        request: Some(processing_request::Request::ResponseHeaders(HttpHeaders {
            headers: Some(EnvoyHeaderMap {
                headers: vec![
                    EnvoyHeaderValue {
                        key: ":status".to_string(),
                        value: "200".to_string(),
                        raw_value: b"200".to_vec(),
                    },
                    EnvoyHeaderValue {
                        key: "content-type".to_string(),
                        value: "".to_string(),
                        raw_value: b"application/json".to_vec(),
                    },
                ],
            }),
            end_of_stream: false,
            ..Default::default()
        })),
        ..Default::default()
    };
    tx.send(headers_msg).await.map_err(|e| format!("send headers: {e}"))?;

    // Generate random body
    let body = {
        let mut rng = rand::rng();
        let body_size = rng.random_range(min_body..=max_body);
        let mut buf = vec![0u8; body_size];
        rng.fill(&mut buf[..]);
        buf
    };

    // Send ResponseBody
    let body_msg = ProcessingRequest {
        request: Some(processing_request::Request::ResponseBody(HttpBody {
            body,
            end_of_stream: true,
            ..Default::default()
        })),
        ..Default::default()
    };
    tx.send(body_msg).await.map_err(|e| format!("send body: {e}"))?;

    // Drop sender so Signet sees end of stream
    drop(tx);

    // Read responses and check for signature headers
    let mut signed = false;
    while let Some(resp) = response_stream
        .message()
        .await
        .map_err(|e| format!("recv: {e}"))?
    {
        if let Some(response) = resp.response {
            let common = match response {
                processing_response::Response::ResponseHeaders(h) => h.response,
                processing_response::Response::ResponseBody(b) => b.response,
                _ => None,
            };
            if let Some(common) = common {
                if let Some(mutation) = common.header_mutation {
                    for hvo in &mutation.set_headers {
                        if let Some(h) = &hvo.header {
                            if h.key == "signature" || h.key == "signature-input" {
                                signed = true;
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(RequestResult {
        duration: start.elapsed(),
        signed,
    })
}

async fn run_bench(addr: &str, concurrency: usize, requests: usize, min_body: usize, max_body: usize) -> RunStats {
    let client = match ExternalProcessorClient::connect(addr.to_string()).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to connect to {addr}: {e}");
            return RunStats {
                total: requests,
                signed: 0,
                errors: requests,
                p50: Duration::ZERO,
                p90: Duration::ZERO,
                p99: Duration::ZERO,
                max: Duration::ZERO,
                throughput: 0.0,
                wall_time: Duration::ZERO,
            };
        }
    };

    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut handles = Vec::with_capacity(requests);
    let overall_start = Instant::now();

    for _ in 0..requests {
        let sem = semaphore.clone();
        let client = client.clone();

        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            run_one_request(client, min_body, max_body).await
        }));
    }

    let mut latencies = Vec::with_capacity(requests);
    let mut signed_count: usize = 0;
    let mut error_count: usize = 0;

    for handle in handles {
        match handle.await.unwrap() {
            Ok(result) => {
                if result.signed {
                    signed_count += 1;
                }
                latencies.push(result.duration);
            }
            Err(e) => {
                error_count += 1;
                eprintln!("Error: {e}");
            }
        }
    }

    let wall_time = overall_start.elapsed();

    if latencies.is_empty() {
        return RunStats {
            total: requests,
            signed: 0,
            errors: error_count,
            p50: Duration::ZERO,
            p90: Duration::ZERO,
            p99: Duration::ZERO,
            max: Duration::ZERO,
            throughput: 0.0,
            wall_time,
        };
    }

    latencies.sort();
    let n = latencies.len();
    let p50 = latencies[n * 50 / 100];
    let p90 = latencies[n * 90 / 100];
    let p99 = latencies[(n - 1) * 99 / 100];
    let max = latencies[n - 1];
    let throughput = requests as f64 / wall_time.as_secs_f64();

    RunStats {
        total: requests,
        signed: signed_count,
        errors: error_count,
        p50,
        p90,
        p99,
        max,
        throughput,
        wall_time,
    }
}

fn fmt_ms(d: Duration) -> String {
    format!("{:.1}ms", d.as_secs_f64() * 1000.0)
}

fn fmt_bytes(n: usize) -> String {
    match n {
        _ if n >= 1_048_576 => format!("{}MB", n / 1_048_576),
        _ if n >= 1_024 => format!("{}KB", n / 1_024),
        _ => format!("{}B", n),
    }
}

const BODY_SIZE_BANDS: &[usize] = &[
    256,                // 256B
    1_024,              // 1KB
    4_096,              // 4KB
    16_384,             // 16KB
    65_536,             // 64KB
    262_144,            // 256KB
    1_048_576,          // 1MB
    5_242_880,          // 5MB
];

fn print_stats(stats: &RunStats) {
    println!(
        "Requests:    {} total, {} signed, {} errors",
        stats.total, stats.signed, stats.errors
    );
    println!(
        "Latency:     p50={}  p90={}  p99={}  max={}",
        fmt_ms(stats.p50), fmt_ms(stats.p90), fmt_ms(stats.p99), fmt_ms(stats.max)
    );
    println!("Throughput:  {:.0} req/s", stats.throughput);
    println!("Duration:    {:.2?}", stats.wall_time);
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    if args.sweep_body {
        eprintln!(
            "Body sweep: {} requests per band, concurrency {}",
            args.requests, args.concurrency
        );
        eprintln!();
        println!(
            "{:>8}  {:>8}  {:>8}  {:>8}  {:>8}  {:>10}  {:>7}  {:>6}",
            "body", "p50", "p90", "p99", "max", "req/s", "signed", "errors"
        );
        println!("{}", "-".repeat(80));

        for &size in BODY_SIZE_BANDS {
            let stats = run_bench(&args.addr, args.concurrency, args.requests, size, size).await;
            println!(
                "{:>8}  {:>8}  {:>8}  {:>8}  {:>8}  {:>10.0}  {:>7}  {:>6}",
                fmt_bytes(size),
                fmt_ms(stats.p50),
                fmt_ms(stats.p90),
                fmt_ms(stats.p99),
                fmt_ms(stats.max),
                stats.throughput,
                stats.signed,
                stats.errors,
            );
        }
    } else if let Some(threshold_ms) = args.sweep {
        let threshold = Duration::from_secs_f64(threshold_ms / 1000.0);
        eprintln!(
            "Sweep mode: escalating concurrency until p50 >= {}",
            fmt_ms(threshold)
        );
        eprintln!(
            "Target: {} requests per level, body {}–{} bytes",
            args.requests, args.min_body, args.max_body
        );
        eprintln!();
        println!(
            "{:>6}  {:>8}  {:>8}  {:>8}  {:>8}  {:>10}  {:>7}  {:>6}",
            "conc", "p50", "p90", "p99", "max", "req/s", "signed", "errors"
        );
        println!("{}", "-".repeat(78));

        let mut c = 1;
        loop {
            let stats = run_bench(&args.addr, c, args.requests, args.min_body, args.max_body).await;
            println!(
                "{:>6}  {:>8}  {:>8}  {:>8}  {:>8}  {:>10.0}  {:>7}  {:>6}",
                c,
                fmt_ms(stats.p50),
                fmt_ms(stats.p90),
                fmt_ms(stats.p99),
                fmt_ms(stats.max),
                stats.throughput,
                stats.signed,
                stats.errors,
            );

            if stats.p50 >= threshold {
                eprintln!();
                eprintln!(
                    "Threshold reached: p50={} >= {} at concurrency {}",
                    fmt_ms(stats.p50), fmt_ms(threshold), c
                );
                break;
            }

            if c >= 4096 {
                eprintln!();
                eprintln!("Reached max concurrency (4096) without hitting threshold");
                break;
            }

            c = (c * 2).min(4096);
        }
    } else {
        eprintln!(
            "Running {} requests with concurrency {} against {}",
            args.requests, args.concurrency, args.addr
        );
        eprintln!(
            "Body size: {}–{} bytes",
            args.min_body, args.max_body
        );

        let stats = run_bench(&args.addr, args.concurrency, args.requests, args.min_body, args.max_body).await;

        if stats.signed == 0 && stats.errors == stats.total {
            eprintln!("All requests failed.");
            return;
        }

        print_stats(&stats);
    }
}
