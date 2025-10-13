# Build stage
FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev sqlite-dev openssl-dev openssl-libs-static pkgconfig

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy SQLx offline cache (for compile-time query checking)
COPY .sqlx ./.sqlx

# Copy source
COPY src ./src
COPY templates ./templates
COPY static ./static
COPY migrations ./migrations

# Build for release
RUN cargo build --release --bin saramcp

# Runtime stage
FROM alpine:latest

RUN apk add --no-cache sqlite-libs ca-certificates

WORKDIR /app

# Copy binary
COPY --from=builder /app/target/release/saramcp /app/saramcp

# Copy templates, static, and migrations
COPY templates ./templates
COPY static ./static
COPY migrations ./migrations

# Create data directory
RUN mkdir -p /app/data

EXPOSE 8080

CMD ["/app/saramcp"]
