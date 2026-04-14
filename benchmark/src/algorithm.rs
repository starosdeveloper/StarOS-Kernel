// Общий алгоритм для бенчмарка
// Матричное умножение 512x512 - чистая математика, CPU-bound

#![no_std]

pub const MATRIX_SIZE: usize = 512;
pub const ITERATIONS: usize = 10;

pub type Matrix = [[f64; MATRIX_SIZE]; MATRIX_SIZE];

#[inline(never)]
pub fn matrix_multiply(a: &Matrix, b: &Matrix, result: &mut Matrix) {
    for i in 0..MATRIX_SIZE {
        for j in 0..MATRIX_SIZE {
            let mut sum = 0.0;
            for k in 0..MATRIX_SIZE {
                sum += a[i][k] * b[k][j];
            }
            result[i][j] = sum;
        }
    }
}

#[inline(never)]
pub fn init_matrix(matrix: &mut Matrix, seed: f64) {
    for i in 0..MATRIX_SIZE {
        for j in 0..MATRIX_SIZE {
            matrix[i][j] = (i as f64 * seed + j as f64) % 100.0;
        }
    }
}

#[inline(never)]
pub fn checksum(matrix: &Matrix) -> f64 {
    let mut sum = 0.0;
    for i in 0..MATRIX_SIZE {
        for j in 0..MATRIX_SIZE {
            sum += matrix[i][j];
        }
    }
    sum
}
