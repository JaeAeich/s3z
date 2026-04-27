# Changelog

All notable changes to this project will be documented in this file.

## [0.1.0-rc.2] — 2026-04-27

### Bug Fixes

- **core:** Remove redundant num_parts and fix private doc links ([e18aaba](https://github.com/jaeaeich/s3z/commit/e18aaba4a33f183149e88f6c39e4e6edbd2a63c3))

### CI/CD

- Add Rust crate release workflow ([9826ec2](https://github.com/jaeaeich/s3z/commit/9826ec2cd679b38a99083ac1eab6b6104fd2375d))
- Add PR and push-to-main CI workflow ([215bd72](https://github.com/jaeaeich/s3z/commit/215bd7217ecb93dbe87850a2e217bb6419241703))
- **deps:** Bump actions/attest-build-provenance from 3 to 4 ([baa4782](https://github.com/jaeaeich/s3z/commit/baa47829c4e22a9f48538630f1f48ee96037114d))
- **deps:** Bump actions/setup-node from 4 to 6 (#8) ([63a975c](https://github.com/jaeaeich/s3z/commit/63a975cedad6ca60b789846407882f12c21c5f32))
- **deps:** Bump actions/cache from 4 to 5 (#7) ([ae81a29](https://github.com/jaeaeich/s3z/commit/ae81a29484e102af814a501635dcabc2d54cfd57))

### Documentation

- **bindings:** Add package READMEs for PyPI and npm ([032ceba](https://github.com/jaeaeich/s3z/commit/032cebaf3bde36b2f0ee359112208c3f971df029))
- **bindings:** Add install options and fix endpoint labels ([dd9886c](https://github.com/jaeaeich/s3z/commit/dd9886cb103f04c010d2c428e320f096422c9a79))
- **core:** Add crate-specific README for crates.io ([ba23b83](https://github.com/jaeaeich/s3z/commit/ba23b83d0cfb6cdbd6549b4bde7498e09cfc6c2a))
- Add fumadocs documentation site ([2b54199](https://github.com/jaeaeich/s3z/commit/2b541999c98008067ad6e771899c6d199f492780))
- Link example servers from quick-start pages ([0375ace](https://github.com/jaeaeich/s3z/commit/0375ace50536d38acfff7f464a849edbdccb2610))

### Features

- **bench:** Add dark theme variant for benchmark charts ([cf3d24c](https://github.com/jaeaeich/s3z/commit/cf3d24ca32b44acdc5b2b780a31f120b0b2616a1))

### Miscellaneous

- Track node binding type declarations for docs ([c85af29](https://github.com/jaeaeich/s3z/commit/c85af2914eeb8196255c4365ae19ae58f18d6028))
- Update lockfiles ([7d3d066](https://github.com/jaeaeich/s3z/commit/7d3d066e65fa33c7cbd89994cbf7fac21b836e3a))
- Add ruff hooks and scope editorconfig glob in lefthook ([3bd1e64](https://github.com/jaeaeich/s3z/commit/3bd1e64d5c30f8db5ffcbf54415aa1f23c8f3870))
- Bump rust to 1.95.0 and pin in CI ([27b23e0](https://github.com/jaeaeich/s3z/commit/27b23e04992ec4798fc479532645255dd7930b57))

### Style

- **bench:** Apply ruff formatting to plot.py ([51985c7](https://github.com/jaeaeich/s3z/commit/51985c7332ffafc4109006af326043b195dffb72))
## [0.1.0-rc.1] — 2026-04-27

### Bug Fixes

- Set explicit Content-Length and sign empty bodies correctly ([63ff803](https://github.com/jaeaeich/s3z/commit/63ff803e6838bd838ec98ad6af8b41495e07ef4a))
- Multipart race, stale SigV4 on retry, and hardening ([c90f1c7](https://github.com/jaeaeich/s3z/commit/c90f1c76eb016846e70d2bf982fb8ca40dda02c6))
- **bench:** Mc region signing, dev profile tuning, dynamic plot labels ([0083423](https://github.com/jaeaeich/s3z/commit/008342303b68fdf63cb079b9b38b3bbcd58cf0f4))
- **core:** Panic on zero workers/concurrency, fix misleading docs ([9bbdf29](https://github.com/jaeaeich/s3z/commit/9bbdf29a3b90192a9983cc99d1c1177b2fd4cf5e))
- **core:** Wrap long macro line and set garage region to us-east-1 ([ee33dc3](https://github.com/jaeaeich/s3z/commit/ee33dc3ab6635d1be84033569aa4e20581fb1487))

### Build

- **core:** Add futures-util and tempfile deps, wire download exports ([63291af](https://github.com/jaeaeich/s3z/commit/63291af721827ef00b2387c994063a8057600494))
- **workspace:** Upgrade deps and add FFI workspace dependencies ([b465836](https://github.com/jaeaeich/s3z/commit/b4658366ef471ff3c548eeceb83d8dbe15d3f703))
- **node:** Configure napi-rs targets and scoped npm package ([78f3a13](https://github.com/jaeaeich/s3z/commit/78f3a13338324fdf2ef5565bfd07a36d8f3c907d))

### CI/CD

- Add Python release workflow with maturin ([d04ddd6](https://github.com/jaeaeich/s3z/commit/d04ddd6001dfc51ee598345967d75d4253666233))
- Add Node release workflow with napi-rs ([74c6889](https://github.com/jaeaeich/s3z/commit/74c68896a2d8195d6db28c2a9eaa9c321eadd847))
- Add version bump and release workflow ([ba9a53b](https://github.com/jaeaeich/s3z/commit/ba9a53bafbe41c458139a1a4986167917d58fccc))

### Documentation

- Add benchmark plots to README, save full run ([65baf22](https://github.com/jaeaeich/s3z/commit/65baf2235b99f2855c66bb8c7ade9b78752ef22b))
- Add GitHub community health files and Dependabot ([9c65146](https://github.com/jaeaeich/s3z/commit/9c65146d2b46665f5df9ab78ae8d6062c96d1206))
- Add CLI install section to README ([a565275](https://github.com/jaeaeich/s3z/commit/a5652750f3281b8910058e00115e4f10552c27d1))

### Features

- Add core types, config, auth, and error handling ([1d7ee4e](https://github.com/jaeaeich/s3z/commit/1d7ee4e4cee89ea16f8d6d0fdd6720828038df1a))
- Add HTTP layer — ObjectKey encoding, request signing, response parsing, retry ([8c53943](https://github.com/jaeaeich/s3z/commit/8c53943f0a6e70d58b7240d5a08c95f3234c61b8))
- Add transfer engine and upload operation ([c5aa1d4](https://github.com/jaeaeich/s3z/commit/c5aa1d425eac001c7e9234b837ed4bfd4dfc39d7))
- Add real-time upload progress with indicatif ([78891ab](https://github.com/jaeaeich/s3z/commit/78891ab8b044d4e9d869d156cd96e9bde9a6b3c5))
- Add brand assets and README ([529a5aa](https://github.com/jaeaeich/s3z/commit/529a5aa56b72316e227ed20f3c2990744af4b153))
- Add docker compose with four S3-compatible backends ([630a23b](https://github.com/jaeaeich/s3z/commit/630a23bf2956da631582c85d1f53e108d2a2c3c7))
- Add optional tracing instrumentation and review fixes ([bbae045](https://github.com/jaeaeich/s3z/commit/bbae045e0088a6554149aa0ac722caf0b35ab4fb))
- **bench:** Add benchmark framework with pluggable tools and operations ([002e7a0](https://github.com/jaeaeich/s3z/commit/002e7a00535366647e1f3f18aa129b41d99184d8))
- **bench:** Regression detection and pluggable operations ([85ee761](https://github.com/jaeaeich/s3z/commit/85ee76130a4bfc05c43adb9537fc176f9702ad1c))
- **transfer:** Dynamic part sizing based on file size and concurrency ([e3ae055](https://github.com/jaeaeich/s3z/commit/e3ae055657bfa583d350a73abc1cd6a20cfb3b6f))
- **core:** Add ListObjectsV2 XML response parsing ([072cf58](https://github.com/jaeaeich/s3z/commit/072cf584ff1fe43d983c658ec3fc6d7ca0e9885c))
- **core:** Add paginated list API with owned ListPaginator ([2b0548f](https://github.com/jaeaeich/s3z/commit/2b0548feb53e071de02f8c2987d5db6fc7e00382))
- **cli:** Add ls command for listing S3 objects ([ace5784](https://github.com/jaeaeich/s3z/commit/ace57846e93cdbf39fa490f68b234d64aae5db8f))
- **bench:** Add list operation, run all tools on defaults ([3d108dc](https://github.com/jaeaeich/s3z/commit/3d108dce7794acf59d3de9225a3a4149e005bfad))
- **transfer:** Add single GET and Range-based multipart download ([7ca495f](https://github.com/jaeaeich/s3z/commit/7ca495f47ee3e64268844ec430f94412693c019a))
- **transfer:** Add channel-driven work pool for pipelined downloads ([f1b7c31](https://github.com/jaeaeich/s3z/commit/f1b7c311099c8dfc15247519a8f011e02d701f1a))
- **core:** Add batch download API with auto-tuned parallelism ([8d8e9b4](https://github.com/jaeaeich/s3z/commit/8d8e9b4d5c107a9745a3f3ea7874cac92186793d))
- **cli:** Add download command with progress bar and auto-tuning ([f4b3878](https://github.com/jaeaeich/s3z/commit/f4b38785a6e313f4025e313568594eed01788261))
- **bench:** Add download operation with plotting and tool commands ([207c38d](https://github.com/jaeaeich/s3z/commit/207c38d013b885caa8d1aea6189f484ecad2e9a6))
- **core:** Add separate multipart download threshold ([f812bea](https://github.com/jaeaeich/s3z/commit/f812beaf4b527948a54dd96326bb318b91f5468d))
- **python:** Add PyO3 bindings for s3z ([90b3deb](https://github.com/jaeaeich/s3z/commit/90b3debd6d0bad7b13c6869046979b736742094b))
- **node:** Add NAPI-RS bindings for s3z ([6a8ffd8](https://github.com/jaeaeich/s3z/commit/6a8ffd8e3586ff17257df71b0f0359c7f985cb03))
- **examples:** Add FastAPI server with Scalar docs ([ccb0015](https://github.com/jaeaeich/s3z/commit/ccb0015990e9912a12cfcff255d4af4e374465fb))
- **examples:** Add Elysia server with Scalar docs ([6445b25](https://github.com/jaeaeich/s3z/commit/6445b2540b35117ceb31097706f316a3a181d8e8))
- **examples:** Add Axum server with Scalar docs ([acbaea9](https://github.com/jaeaeich/s3z/commit/acbaea939b783d2aeef8af30243345847fd03697))

### Miscellaneous

- Init ([65fe070](https://github.com/jaeaeich/s3z/commit/65fe070d35ed64afd3b0a8b48b8561fa9d6335db))
- **bench:** Remove rustfs due to high instability ([55d77d3](https://github.com/jaeaeich/s3z/commit/55d77d3ba7060e949134b65246c8caf0f5884da1))
- Add docker-compose services, mise stubs task, and gitignore ([a695549](https://github.com/jaeaeich/s3z/commit/a695549347047e70278bfb760cc1cc6f30d0079b))
- Drop Windows from CLI release targets ([6b1faa0](https://github.com/jaeaeich/s3z/commit/6b1faa0b9d325ad820721f159000a2f2e89ce714))
- Add git-cliff changelog configuration ([3d896fe](https://github.com/jaeaeich/s3z/commit/3d896feb2ceb16e6e94b0419570aef80b7bb429e))
- **release:** V0.1.0-rc.1 ([eb30cfc](https://github.com/jaeaeich/s3z/commit/eb30cfc92d274af0d5d0cbaf14f94e8ab59cd9b4))

### Performance

- **core:** Fix worker starvation, add retry jitter, tune TCP ([71773f8](https://github.com/jaeaeich/s3z/commit/71773f8ac4c7a0c0422de67015efc6b0cb67a214))
- **core:** Warm connection pool automatically on client init ([f445a68](https://github.com/jaeaeich/s3z/commit/f445a684fb25a88eb5d8655d38670beb9380e741))
- **core:** Stream single-put uploads from disk instead of buffering ([62aeede](https://github.com/jaeaeich/s3z/commit/62aeedec15de3e46b8cdc29106e7359714e068e0))
- **core:** Remove eager connection pool warmup ([39cf333](https://github.com/jaeaeich/s3z/commit/39cf333720bc2f606d53649ebfb5e63f4a37c037))
- **transfer:** Buffer pwrite calls and uncap download part size ([ac47c1f](https://github.com/jaeaeich/s3z/commit/ac47c1fe1313c182c8104892ba4e99801d9cf620))
- **transfer:** Dynamic per-file concurrency based on size ([c8e27aa](https://github.com/jaeaeich/s3z/commit/c8e27aa06e1ad3170cc04b902ef048fd58899baa))
- **transfer:** Replace spawn_blocking with dedicated writer thread ([974411a](https://github.com/jaeaeich/s3z/commit/974411a52ce0b1fc306fdb9789671160328b9c99))

### Refactoring

- Restructure into workspace with CLI ([f56e37a](https://github.com/jaeaeich/s3z/commit/f56e37aaf8942dc9358b99b36aa7ecff070ace28))
- Replace blanket clippy::restriction with individual lint opt-ins ([d2533ac](https://github.com/jaeaeich/s3z/commit/d2533ac8e7ad4e4d61430b0fd2c909044f35a3f1))
- **transfer:** Extract bounded work pool from upload fan-out ([5677f76](https://github.com/jaeaeich/s3z/commit/5677f76208fce5cca6160148f811356ff01a0e04))
- **transfer:** Split part sizing for upload and download ([f2a72e5](https://github.com/jaeaeich/s3z/commit/f2a72e5fca7f8425e2990aba454a2ac70ce8cc94))
- Move bindings to separate workspace and add cargo-dist ([2b9b648](https://github.com/jaeaeich/s3z/commit/2b9b6482e76824087258d403d9b9e2c4edde54ca))

