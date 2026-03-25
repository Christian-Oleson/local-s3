<?xml version="1.0" encoding="UTF-8"?>
<!-- Dos Apes Super Agent Framework - Phase Plan -->
<!-- Generated: 2026-03-25 -->
<!-- Phase: 5 -->

<plan>
  <metadata>
    <phase>5</phase>
    <name>Docker, CI, Polish</name>
    <goal>Production-quality Docker image, CI pipeline, documentation, and final polish</goal>
    <deliverable>`docker run` one-liner that replaces LocalStack S3 for any developer</deliverable>
    <created>2026-03-25</created>
  </metadata>

  <context>
    <dependencies>Phase 4 complete — all S3 features implemented, 189 tests passing</dependencies>
    <affected_areas>
      - Dockerfile — multi-stage build
      - docker-compose.yml — example with volume mount
      - .github/workflows/ — CI pipeline
      - README.md — quickstart, examples, SDK config snippets
      - src/main.rs — health check, --log-level arg
      - src/server.rs — health check route
      - CLAUDE.md — update with final architecture
    </affected_areas>
    <patterns_to_follow>
      - Already have: CLI args (--port, --data-dir), tracing with env filter, release profile (LTO, strip, panic=abort)
      - Target: musl for static linking in Docker (x86_64-unknown-linux-musl)
      - Docker image: multi-stage build, scratch or alpine base
      - Port 4566 default (same as LocalStack)
      - Health check at GET / returns ListBuckets (already works)
    </patterns_to_follow>
  </context>

  <tasks>
    <task id="1" type="setup" complete="false">
      <name>Dockerfile, docker-compose, health check, CLI polish</name>
      <description>
        Create production Docker image, docker-compose example, add explicit health check
        endpoint, and add --log-level CLI argument. The Dockerfile uses multi-stage build
        with musl for static linking to produce a minimal image.
      </description>

      <files>
        <create>Dockerfile                    — multi-stage: rust builder → scratch runtime</create>
        <create>docker-compose.yml            — example with volume mount and port mapping</create>
        <create>.dockerignore                 — exclude target/, .git/, data/, tests/</create>
        <modify>src/main.rs                   — add --log-level arg</modify>
        <modify>src/server.rs                 — add explicit health check GET / endpoint (return 200 OK)</modify>
      </files>

      <action>
        1. Create Dockerfile:
           ```dockerfile
           # Stage 1: Build
           FROM rust:1.94-slim AS builder
           WORKDIR /app
           COPY Cargo.toml Cargo.lock ./
           COPY src/ src/
           RUN cargo build --release

           # Stage 2: Runtime
           FROM debian:bookworm-slim
           RUN apt-get update &amp;&amp; apt-get install -y ca-certificates &amp;&amp; rm -rf /var/lib/apt/lists/*
           COPY --from=builder /app/target/release/local-s3 /usr/local/bin/local-s3

           EXPOSE 4566
           VOLUME /data

           ENTRYPOINT ["local-s3"]
           CMD ["--port", "4566", "--data-dir", "/data"]
           ```

           Note: Using debian-slim instead of scratch/musl because musl cross-compilation from
           Windows is complex and not critical for a local dev tool. Can be optimized later.

        2. Create docker-compose.yml:
           ```yaml
           services:
             local-s3:
               build: .
               ports:
                 - "4566:4566"
               volumes:
                 - ./data:/data
               environment:
                 - RUST_LOG=local_s3=info
           ```

        3. Create .dockerignore:
           ```
           target/
           .git/
           data/
           tests/
           .planning/
           .claude/
           *.md
           ```

        4. Update src/main.rs:
           - Add --log-level CLI arg (trace, debug, info, warn, error) defaulting to "info"
           - Apply via EnvFilter

        5. Update src/server.rs:
           - The root GET / route already returns ListBuckets, which serves as a health check
           - Add a simple GET /_health route that returns 200 OK with "healthy" body
             (Docker HEALTHCHECK can use this)

        6. The Cargo.lock is gitignored — for Docker builds we need it.
           Remove Cargo.lock from .gitignore so it's committed.
           Run `cargo generate-lockfile` to create it if missing.
      </action>

      <verification>
        <command>cargo build --release</command>
        <command>cargo test</command>
        <manual>
          docker build -t local-s3 .
          docker run -p 4566:4566 -v ./data:/data local-s3
          curl http://localhost:4566/_health → "healthy"
          curl http://localhost:4566 → ListBuckets XML
        </manual>
      </verification>

      <done>
        - Dockerfile builds successfully
        - docker-compose.yml exists with volume mount
        - Health check endpoint works
        - --log-level CLI arg works
        - All 189 tests still pass
      </done>
    </task>

    <task id="2" type="setup" complete="false">
      <name>GitHub Actions CI and README documentation</name>
      <description>
        Create GitHub Actions workflow for CI (build, test, clippy, fmt).
        Write comprehensive README with quickstart, docker-compose example,
        and SDK configuration snippets for AWS CLI, Node.js, Python, and Rust.
      </description>

      <files>
        <create>.github/workflows/ci.yml      — CI pipeline: build, test, lint, Docker</create>
        <modify>README.md                      — complete documentation</modify>
        <modify>CLAUDE.md                      — update with final architecture</modify>
      </files>

      <action>
        1. Create .github/workflows/ci.yml:
           - Trigger on push to main and PRs
           - Jobs:
             a) check: cargo fmt --check, cargo clippy -- -D warnings
             b) test: cargo test (with timeout)
             c) build-release: cargo build --release
             d) docker: docker build (only on main push)
           - Use actions/checkout, dtolnay/rust-toolchain
           - Cache cargo registry and target

        2. Rewrite README.md:
           - Project name + one-line description
           - Why (LocalStack account requirement problem)
           - Quickstart: docker run one-liner
           - docker-compose example
           - Binary usage (cargo install / cargo run)
           - SDK Configuration snippets:
             * AWS CLI: --endpoint-url http://localhost:4566
             * Node.js (aws-sdk-v3): endpoint, forcePathStyle
             * Python (boto3): endpoint_url
             * Rust (aws-sdk-s3): endpoint_url, force_path_style
           - Supported S3 Operations (table of what's implemented)
           - Configuration (env vars, CLI args)
           - Persistence (data directory, volume mounts)
           - License (MIT)

        3. Update CLAUDE.md with final architecture notes reflecting all 5 phases.
      </action>

      <verification>
        <manual>
          Review README.md for completeness
          Review CI workflow for correctness
          Verify CLAUDE.md is up to date
        </manual>
      </verification>

      <done>
        - CI workflow runs build, test, lint, Docker
        - README has quickstart, SDK examples, feature table
        - CLAUDE.md updated with final architecture
      </done>
    </task>
  </tasks>

  <phase_verification>
    <commands>
      <command>cargo build --release</command>
      <command>cargo test</command>
      <command>cargo fmt -- --check</command>
      <command>cargo clippy -- -D warnings</command>
    </commands>
    <manual>
      1. docker build -t local-s3 .
      2. docker run -p 4566:4566 -v ./data:/data local-s3
      3. curl http://localhost:4566/_health → healthy
      4. AWS CLI: aws --endpoint-url http://localhost:4566 s3 mb s3://test
      5. Verify README has all sections
    </manual>
  </phase_verification>

  <completion_criteria>
    <criterion>Both tasks marked complete</criterion>
    <criterion>Docker image builds and runs</criterion>
    <criterion>CI workflow file exists and is valid</criterion>
    <criterion>README covers quickstart, SDK config, feature table</criterion>
    <criterion>All 189 tests still pass</criterion>
  </completion_criteria>
</plan>
