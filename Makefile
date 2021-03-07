.PHONY: build-docker-image

build-docker-image:
	docker run --rm -it -v "$(shell pwd):/home/rust/src" ekidd/rust-musl-builder:1.49.0 cargo build --release
