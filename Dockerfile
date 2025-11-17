# Build stage
FROM rust:1.91-bookworm AS builder

# Install protobuf compiler
RUN apt-get update && \
    apt-get install -y protobuf-compiler libasound2-dev alsa-oss && \
    rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /usr/src/conclave

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy proto files (needed for build.rs)
COPY proto ./proto

# Copy build script
COPY build.rs ./

# Copy source code
COPY src ./src

# Build the application in release mode
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies (OpenSSL, CA certificates)
RUN apt-get update && \
    apt-get install -y ca-certificates libssl3 && \
    rm -rf /var/lib/apt/lists/*

# Create a non-root user
RUN useradd -m -u 1000 conclave && \
    mkdir -p /home/conclave/.conclave && \
    chown -R conclave:conclave /home/conclave

# Copy the binary from builder
COPY --from=builder /usr/src/conclave/target/release/conclave /usr/local/bin/conclave

# Set the user
USER conclave
WORKDIR /home/conclave

# Set the entrypoint
ENTRYPOINT ["/usr/local/bin/conclave"]

# Default command (can be overridden)
CMD ["--help"]
