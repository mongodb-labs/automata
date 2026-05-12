FROM rust:1.94-slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main(){}" > src/main.rs \
    && cargo build --release \
    && rm -f target/release/automata target/release/deps/automata*
COPY src ./src
RUN cargo build --release

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /app/target/release/automata /automata
COPY automations /automations
ENV PORT=8080
EXPOSE 8080
ENTRYPOINT ["/automata", "/automations"]
