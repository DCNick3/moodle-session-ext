# syntax = docker/dockerfile:1.2

FROM bash AS get-tini

# Add Tini init-system
ENV TINI_VERSION v0.19.0
ADD https://github.com/krallin/tini/releases/download/${TINI_VERSION}/tini-static /tini
RUN chmod +x /tini

FROM bash AS get-protoc

# Add Tini init-system
ENV PROTOC_VERSION 21.6

RUN wget https://github.com/protocolbuffers/protobuf/releases/download/v${PROTOC_VERSION}/protoc-${PROTOC_VERSION}-linux-x86_64.zip -O /protoc.zip && \
    unzip /protoc.zip -d /protoc_zip && \
    mv /protoc_zip/bin/protoc /protoc && \
    chmod +x /protoc && \
    rm -rf /protoc_zip /protoc.zip

FROM clux/muslrust:stable as chef
RUN cargo install cargo-chef --locked

FROM chef as planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build

ARG project_name

COPY --from=get-protoc /protoc /usr/local/bin/protoc

COPY --from=planner /volume/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN \
    cargo chef cook --profile ship --target x86_64-unknown-linux-musl --recipe-path recipe.json
# Build application
COPY . .
RUN \
    cargo b --profile ship --target x86_64-unknown-linux-musl && \
    cp target/x86_64-unknown-linux-musl/ship/moodle-session-ext moodle-session-ext

FROM gcr.io/distroless/static

ARG project_name
ARG repo_name

LABEL org.opencontainers.image.source=https://github.com/DCNick3/moodak
EXPOSE 8080

ENV ENVIRONMENT=prod

COPY --from=get-tini /tini /tini
COPY --from=build /volume/moodle-session-ext /moodle-session-ext
COPY config.prod.yml /config.yml

ENTRYPOINT ["/tini", "--", "/moodle-session-ext"]
