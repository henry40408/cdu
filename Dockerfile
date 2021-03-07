FROM scratch

COPY target/x86_64-unknown-linux-musl/release/turbo-spoon /

CMD ["/turbo-spoon"]
