// Linux версия бенчмарка (для Docker)
use std::time::Instant;

mod algorithm;
use algorithm::*;

fn main() {
    println!("=== STAR OS Benchmark - Linux/Docker ===");
    println!("Matrix size: {}x{}", MATRIX_SIZE, MATRIX_SIZE);
    println!("Iterations: {}", ITERATIONS);
    
    let mut a = Box::new([[0.0f64; MATRIX_SIZE]; MATRIX_SIZE]);
    let mut b = Box::new([[0.0f64; MATRIX_SIZE]; MATRIX_SIZE]);
    let mut result = Box::new([[0.0f64; MATRIX_SIZE]; MATRIX_SIZE]);
    
    init_matrix(&mut a, 1.5);
    init_matrix(&mut b, 2.3);
    
    println!("\nStarting benchmark...");
    
    let mut times = Vec::with_capacity(ITERATIONS);
    
    for i in 0..ITERATIONS {
        let start = Instant::now();
        matrix_multiply(&a, &b, &mut result);
        let elapsed = start.elapsed();
        
        times.push(elapsed.as_nanos());
        println!("Iteration {}: {} ns", i + 1, elapsed.as_nanos());
    }
    
    let checksum_val = checksum(&result);
    println!("\nChecksum: {:.2}", checksum_val);
    
    // Статистика
    let total: u128 = times.iter().sum();
    let avg = total / times.len() as u128;
    let min = *times.iter().min().unwrap();
    let max = *times.iter().max().unwrap();
    let jitter = max - min;
    
    println!("\n=== Results ===");
    println!("Average: {} ns ({:.3} ms)", avg, avg as f64 / 1_000_000.0);
    println!("Min: {} ns ({:.3} ms)", min, min as f64 / 1_000_000.0);
    println!("Max: {} ns ({:.3} ms)", max, max as f64 / 1_000_000.0);
    println!("Jitter: {} ns ({:.3} ms)", jitter, jitter as f64 / 1_000_000.0);
    println!("Jitter %: {:.2}%", (jitter as f64 / avg as f64) * 100.0);
    
    // Вывод для парсинга
    println!("\n### METRICS ###");
    println!("PLATFORM=linux");
    println!("AVG_NS={}", avg);
    println!("MIN_NS={}", min);
    println!("MAX_NS={}", max);
    println!("JITTER_NS={}", jitter);
    println!("CHECKSUM={:.2}", checksum_val);
}
