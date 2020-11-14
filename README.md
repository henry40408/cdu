# Turbo Spoon

Turbo Spoon is an daemon and CLI to update DNS record on Cloudflare.

## Usage

Run as Docker container:

```bash
$ docker build -t henry40408/turbo-spoon .
$ docker run -it \
  -e CLOUDFLARE_TOKEN=[your Cloudflare token] \
  -e CLOUDFLARE_ZONE=[name of your zone on Cloudflare] \
  -e CLOUDFLARE_RECORD_NAMES=[name of DNS records on Cloudflare] \
  henry40408/turbo-spoon
```

Run directly:

```bash
$ bundle
$ CLOUDFLARE_TOKEN=[your Cloudflare token] \
  CLOUDFLARE_ZONE=[name of your zone on Cloudflare] \
  CLOUDFLARE_RECORD_NAMES=[name of DNS records on Cloudflare] \
  ruby main.rb daemon
```

Run once:

```bash
CLOUDFLARE_TOKEN=[your Cloudflare token] ruby main.rb update [name of your zone on Cloudflare] [name of DNS records on Cloudflare]
```

For documentation:

```bash
ruby main.rb -h
```

## Contributing

Pull requests are welcome. For major changes, please open an issue first to discuss what you would like to change.

Please make sure to update tests as appropriate.

## License

[MIT](https://choosealicense.com/licenses/mit/)
