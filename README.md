# cdu

![GitHub Workflow](https://github.com/henry40408/cdu/actions/workflows/workflow.yml/badge.svg) ![GitHub](https://img.shields.io/github/license/henry40408/cdu)

**C**loudflare **D**NS record **U**pdate

## Usage

Run as Docker container:

```bash
$ make build-docker-image
$ docker run -it \
  -e CLOUDFLARE_TOKEN=[your Cloudflare token] \
  -e CLOUDFLARE_ZONE=[name of your zone on Cloudflare] \
  -e CLOUDFLARE_RECORDS=[name of DNS records on Cloudflare, separated by comma] \
  henry40408/cdu \
  /cdu
```

Run directly:

```bash
CLOUDFLARE_TOKEN=[your Cloudflare token] \
CLOUDFLARE_ZONE=[name of your zone on Cloudflare] \
CLOUDFLARE_RECORDS=[name of DNS records on Cloudflare, separated by comma] \
cargo run -d
```

Run once:

```bash
CLOUDFLARE_TOKEN=[your Cloudflare token] \
CLOUDFLARE_ZONE=[name of your zone on Cloudflare] \
CLOUDFLARE_RECORDS=[name of DNS records on Cloudflare, separated by comma] \
cargo run
```

For help:

```bash
cargo run -- -h
```

## Contributing

Pull requests are welcome. For major changes, please open an issue first to discuss what you would like to change.

Please make sure to update tests as appropriate.

## License

[MIT](https://choosealicense.com/licenses/mit/)
