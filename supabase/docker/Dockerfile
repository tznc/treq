# syntax=docker/dockerfile:1.7
#
# Fat API image (Alpine): PostgREST + Edge Runtime + nginx, supervised by OpenRC.
# Single-tenant self-hosted — no GoTrue. No Python, no Node.js at runtime.
# Postgres is external (compose sidecar).
#
# Build (context = supabase/):
#   docker build -f supabase/docker/Dockerfile -t treq-supabase:local supabase

ARG ALPINE_IMAGE=alpine:3.23
ARG POSTGREST_IMAGE=postgrest/postgrest:v14.12
ARG EDGE_RUNTIME_IMAGE=supabase/edge-runtime:v1.74.0

FROM ${POSTGREST_IMAGE} AS postgrest
FROM ${EDGE_RUNTIME_IMAGE} AS edge

# -----------------------------------------------------------------------------
# Strip + UPX -9 pack edge-runtime. Tools stay in this stage only.
# -9 is strong compression without the multi-hour --ultra-brute search.
# -----------------------------------------------------------------------------
FROM alpine:3.23 AS edge-pack

RUN apk add --no-cache binutils upx

# Minimal glibc loader/libs so we can smoke-exec the packed binary on musl.
RUN mkdir -p /lib64 /lib/x86_64-linux-gnu
COPY --from=edge /lib64/ld-linux-x86-64.so.2 /lib64/ld-linux-x86-64.so.2
COPY --from=edge /lib/x86_64-linux-gnu/libc.so.6 /lib/x86_64-linux-gnu/
COPY --from=edge /lib/x86_64-linux-gnu/libm.so.6 /lib/x86_64-linux-gnu/
COPY --from=edge /lib/x86_64-linux-gnu/libgcc_s.so.1 /lib/x86_64-linux-gnu/
COPY --from=edge /usr/local/bin/edge-runtime /edge-runtime

RUN set -eux; \
  strip --strip-unneeded /edge-runtime; \
  upx -9 /edge-runtime; \
  upx -t /edge-runtime; \
  /edge-runtime --version; \
  ls -lh /edge-runtime

# -----------------------------------------------------------------------------
# Runtime
# -----------------------------------------------------------------------------
FROM ${ALPINE_IMAGE}

RUN apk add --no-cache \
      bash \
      curl \
      nginx \
      openrc \
      postgresql17-client \
      tini

# OpenRC in Docker (no real init / cgroup hierarchy).
RUN mkdir -p /run/openrc /etc/runlevels/default \
  && touch /run/openrc/softlevel \
  && printf '%s\n' \
       'rc_sys="docker"' \
       'rc_env_allow="*"' \
       'rc_provide="loopback net"' \
       >> /etc/rc.conf

# PostgREST (static)
COPY --from=postgrest /bin/postgrest /usr/local/bin/postgrest

# Edge Runtime is glibc — ship stripped/UPX'd binary + minimal loader/libs.
RUN mkdir -p /lib64 /lib/x86_64-linux-gnu
COPY --from=edge-pack /edge-runtime /usr/local/bin/edge-runtime
COPY --from=edge /lib64/ld-linux-x86-64.so.2 /lib64/ld-linux-x86-64.so.2
COPY --from=edge /lib/x86_64-linux-gnu/libc.so.6 /lib/x86_64-linux-gnu/
COPY --from=edge /lib/x86_64-linux-gnu/libm.so.6 /lib/x86_64-linux-gnu/
COPY --from=edge /lib/x86_64-linux-gnu/libgcc_s.so.1 /lib/x86_64-linux-gnu/

# Treq application layer (build context = supabase/)
COPY migrations /opt/treq/migrations
COPY seed.sql /opt/treq/seed.sql
COPY functions /home/deno/functions
COPY docker/edge-main /home/deno/functions/main
COPY docker/db /opt/treq/db
COPY docker/nginx.conf /etc/nginx/nginx.conf
COPY docker/entrypoint.sh /opt/treq/bin/entrypoint.sh
COPY docker/apply-treq-migrations.sh /opt/treq/bin/apply-treq-migrations.sh

# @supabase/supabase-js resolves via its npm: specifier at runtime.
COPY docker/deno.json /home/deno/functions/deno.json

# OpenRC services + wrappers (source /etc/treq/env)
COPY docker/openrc/postgrest /etc/init.d/postgrest
COPY docker/openrc/edge-runtime /etc/init.d/edge-runtime
COPY docker/openrc/nginx-gateway /etc/init.d/nginx-gateway
COPY docker/openrc/run-postgrest.sh /usr/local/bin/run-postgrest.sh
COPY docker/openrc/run-edge-runtime.sh /usr/local/bin/run-edge-runtime.sh
COPY docker/openrc/run-nginx.sh /usr/local/bin/run-nginx.sh

RUN find /home/deno/functions -name '.env' -delete 2>/dev/null || true \
  && chmod +x \
      /opt/treq/bin/entrypoint.sh \
      /opt/treq/bin/apply-treq-migrations.sh \
      /usr/local/bin/postgrest \
      /usr/local/bin/edge-runtime \
      /etc/init.d/postgrest \
      /etc/init.d/edge-runtime \
      /etc/init.d/nginx-gateway \
      /usr/local/bin/run-postgrest.sh \
      /usr/local/bin/run-edge-runtime.sh \
      /usr/local/bin/run-nginx.sh \
  && rc-update add postgrest default \
  && rc-update add edge-runtime default \
  && rc-update add nginx-gateway default \
  && mkdir -p /var/log/nginx /var/lib/nginx /run/nginx /tmp /root/.cache/deno

ENV POSTGRES_DB=postgres \
    POSTGRES_USER=supabase_admin \
    PGHOST=db \
    PGPORT=5432 \
    JWT_EXP=3600 \
    VERIFY_JWT=false \
    DENO_DIR=/root/.cache/deno

EXPOSE 8000

HEALTHCHECK --interval=10s --timeout=5s --start-period=45s --retries=12 \
  CMD curl -sf http://127.0.0.1:8000/health && curl -sf http://127.0.0.1:3000/ || exit 1

ENTRYPOINT ["/sbin/tini", "--", "/opt/treq/bin/entrypoint.sh"]
