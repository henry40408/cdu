FROM alpine:3

RUN apk add --no-cache ca-certificates

COPY target/x86_64-unknown-linux-musl/release/turbo-spoon /

CMD ["/turbo-spoon"]
