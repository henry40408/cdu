.PHONY: build-docker-image

build-docker-image:
	cross build --release --target x86_64-unknown-linux-musl
	docker build -t henry40408/cdu .
