# NAS-only x86_64 image for CPUs without AVX/AVX2 (Intel J4125).
# This image intentionally does not use the upstream precompiled static ORT.
FROM rust:1.96-trixie AS chef
RUN cargo install cargo-chef --locked
WORKDIR /src

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build-nas
ENV RUSTFLAGS="-C target-cpu=x86-64-v2" \
    CARGO_BUILD_JOBS=4
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release --locked \
    -p docling-serve --features ort-load-dynamic \
    --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked \
    -p docling-serve --features ort-load-dynamic \
    && test -x /src/target/release/docling-serve

FROM debian:trixie-slim AS assets
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl tar gzip \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY scripts/install/download_dependencies.sh scripts/install/download_dependencies.sh
# The repository downloader verifies the pinned ONNX Runtime shared library.
# Skip speech and chunking assets; only document/PDF models are needed.
RUN sh scripts/install/download_dependencies.sh \
      --no-asr --no-chunk --with-onnxruntime \
    && test -s .models/onnxruntime/libonnxruntime.so

FROM debian:trixie-slim AS serve-nas
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       ca-certificates curl libstdc++6 libgomp1 ffmpeg \
       tesseract-ocr tesseract-ocr-eng tesseract-ocr-chi-sim tesseract-ocr-osd \
       fonts-liberation fonts-dejavu-core fonts-noto-cjk \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=assets /src/.models /app/.models
COPY --from=build-nas /src/target/release/docling-serve /usr/local/bin/docling-serve
ENV DOCLING_RS_MODELS_DIR=/app/.models \
    ORT_DYLIB_PATH=/app/.models/onnxruntime/libonnxruntime.so \
    DOCLING_RS_EP=cpu \
    DOCLING_RS_SYSTEM_FONTS=0 \
    DOCLING_RS_FONT_DIRS=/usr/share/fonts/truetype/liberation:/usr/share/fonts/truetype/dejavu:/usr/share/fonts/opentype/noto \
    DOCLING_RS_OUTPUT_DIR=/output
RUN mkdir -p /output \
    && test -s "$ORT_DYLIB_PATH" \
    && tesseract --list-langs 2>&1 | grep -q chi_sim
EXPOSE 5001
HEALTHCHECK --interval=15s --timeout=5s --start-period=180s --retries=3 \
    CMD curl -fsS http://127.0.0.1:5001/ready >/dev/null || exit 1
CMD ["docling-serve", "--addr", "0.0.0.0:5001", "--warmup"]
