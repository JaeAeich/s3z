<p align="center">
  <picture>
    <source
      media="(prefers-color-scheme: dark)"
      srcset="https://raw.githubusercontent.com/JaeAeich/s3z/main/public/banner-dark.svg">
    <source
      media="(prefers-color-scheme: light)"
      srcset="https://raw.githubusercontent.com/JaeAeich/s3z/main/public/banner-cream.svg">
    <img
      alt="s3z — S3 ops, but fearlessly fast"
      src="https://raw.githubusercontent.com/JaeAeich/s3z/main/public/banner-cream.svg"
      width="800">
  </picture>
</p>

<p align="center">
  <strong>S3 ops, but fearlessly fast.</strong><br>
  <sub>Built in Rust. Streaming. Parallel. No bloat.</sub>
</p>

---

Node.js bindings for [s3z](https://github.com/JaeAeich/s3z) — a lightweight,
high-throughput S3 library built in Rust.

## Install

```bash
npm install @jae_aeich/s3z   # npm
pnpm add @jae_aeich/s3z      # pnpm
yarn add @jae_aeich/s3z      # yarn
bun add @jae_aeich/s3z       # bun
```

## Quick start

```typescript
import { S3Client } from "@jae_aeich/s3z";

const client = await S3Client.create({ region: "us-east-1" });

// Upload
const uploaded = await client.upload({
  sources: ["./data"],
  bucket: "my-bucket",
  prefix: "uploads/",
});

// Download
const downloaded = await client.download({
  bucket: "my-bucket",
  prefix: "uploads/",
  destDir: "./out",
});

// List
const objects = await client.list({
  bucket: "my-bucket",
  prefix: "uploads/",
});
```

### S3-compatible backends

```typescript
const client = await S3Client.create({
  region: "us-east-1",
  accessKey: "access-key",
  secretKey: "secret-key",
  endpoint: "http://localhost:9000", // MinIO, SeaweedFS, Garage, etc.
});
```

## License

[MIT](https://github.com/JaeAeich/s3z/blob/main/LICENSE)
