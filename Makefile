.PHONY: clean docker

clean:
	rm target/linux/amd64/cdu target/linux/arm64/cdu

docker: target/linux/amd64/cdu target/linux/arm64/cdu
	docker buildx build --platform linux/amd64,linux/arm64 -t henry40408/cdu .

target/linux/amd64/cdu:
	mkdir -p target/linux/amd64
	cross build --release --target x86_64-unknown-linux-musl
	cp target/x86_64-unknown-linux-musl/release/cdu $@

target/linux/arm64/cdu:
	mkdir -p target/linux/arm64
	cross build --release --target armv7-unknown-linux-musleabihf
	cp target/armv7-unknown-linux-musleabihf/release/cdu $@
