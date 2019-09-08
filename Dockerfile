# rust-musl-cross sets the workdir to /home/rust/src
FROM messense/rust-musl-cross:armv7-musleabihf AS builder
COPY . .
RUN cargo install --target armv7-unknown-linux-musleabihf --path . --root .

FROM arm32v7/alpine
COPY --from=builder /home/rust/src/bin/data-collector .
CMD ["./data-collector"]
