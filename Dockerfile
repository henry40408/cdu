FROM ruby:2.7.2-alpine

COPY Gemfile .
COPY Gemfile.lock .

RUN apk add --no-cache g++ musl-dev make && \
    bundle && \
    apk del g++ musl-dev make

COPY . .

CMD ruby main.rb daemon
