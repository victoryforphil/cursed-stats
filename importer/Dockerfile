FROM rust:1.81-slim as builder

WORKDIR /usr/src/importer
COPY . .

RUN rm -rf Cargo.lock

# Add CACHEBUST argument to force rebuild when needed
ARG CACHEBUST=1
RUN echo "Building with CACHEBUST=${CACHEBUST}"

RUN cargo build --release

# Use a newer Debian version to avoid GLIBC compatibility issues
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /usr/src/importer/target/release/importer /app/importer

ENV INFLUXDB_URL=http://influxdb:8086
ENV INFLUXDB_DB=cursed_stats
ENV INFLUXDB_USER=stats_user
ENV INFLUXDB_PASSWORD=stats_password
ENV SCAN_DIR=/data
ENV MEASUREMENT=stats
ENV SCANNER_THREADS=2
ENV PARSER_THREADS=4
ENV DB_THREADS=4
ENV BUFFER_SIZE=100000
ENV CACHE_FILE=/app/.import_cache.json
ENV LOG_FILE=/app/importer.log

VOLUME ["/data"]

# Create a wrapper script to handle environment variable substitution properly
RUN echo '#!/bin/sh\n\
/app/importer \
--url ${INFLUXDB_URL} \
--db-name ${INFLUXDB_DB} \
--scan-dir ${SCAN_DIR} \
--measurement ${MEASUREMENT} \
--scanner-threads ${SCANNER_THREADS} \
--parser-threads ${PARSER_THREADS} \
--db-threads ${DB_THREADS} \
--buffer-size ${BUFFER_SIZE} \
--cache-file ${CACHE_FILE} \
--log-file ${LOG_FILE} \
--console\
' > /app/entrypoint.sh && chmod +x /app/entrypoint.sh

ENTRYPOINT ["/app/entrypoint.sh"]