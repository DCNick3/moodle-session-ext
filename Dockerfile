# syntax = docker/dockerfile:1.2

ARG HONEYCOMB_API_KEY

FROM clux/muslrust:stable as build

COPY . /volume
RUN --mount=type=cache,target=/root/.cargo/registry --mount=type=cache,target=/volume/target \
    cargo b --profile ship --target x86_64-unknown-linux-musl && \
    cp target/x86_64-unknown-linux-musl/ship/moodle-session-ext moodle-session-ext

FROM gcr.io/distroless/static

ENV HONEYCOMB_API_KEY=$HONEYCOMB_API_KEY
EXPOSE 8080

COPY --from=build /volume/moodle-session-ext /moodle-session-ext
COPY config.prod.yml /config.yml


CMD ["/moodle-session-ext"]
