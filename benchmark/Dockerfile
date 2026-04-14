FROM rust:1.75-alpine

WORKDIR /benchmark

# Копируем исходники
COPY algorithm.rs .
COPY linux_bench.rs .

# Создаем минимальный Cargo.toml
RUN printf '[package]\n\
name = "linux_bench"\n\
version = "0.1.0"\n\
edition = "2021"\n\
\n\
[[bin]]\n\
name = "bench"\n\
path = "linux_bench.rs"\n\
\n\
[profile.release]\n\
opt-level = 3\n\
lto = true\n\
codegen-units = 1\n' > Cargo.toml

# Собираем с максимальной оптимизацией
RUN cargo build --release

CMD ["./target/release/bench"]
