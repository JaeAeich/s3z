import { swagger } from "@elysiajs/swagger";
import { Elysia } from "elysia";
import { routes } from "./routes";

const app = new Elysia()
  .use(
    swagger({
      documentation: {
        info: {
          title: "s3z",
          version: "0.1.0",
          description:
            "A thin REST layer over s3z's Node.js bindings. " +
            "All heavy lifting (multipart upload/download, connection pooling, SigV4 signing) " +
            "happens in the native Rust library; this server just exposes it over HTTP.",
        },
        tags: [
          {
            name: "s3",
            description: "S3 operations powered by s3z.",
          },
        ],
      },
      path: "/docs",
      provider: "scalar",
    }),
  )
  .use(routes)
  .listen(3000);

console.log(`s3z elysia server running at ${app.server?.url}`);
