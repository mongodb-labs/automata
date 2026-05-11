FROM rust:1.94-slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release && rm src/main.rs
COPY src ./src
COPY automations ./automations
RUN touch src/main.rs && cargo build --release

FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=builder /app/target/release/automata /automata
COPY --from=builder /app/automations /automations
EXPOSE 8080
ENTRYPOINT ["/automata"]
