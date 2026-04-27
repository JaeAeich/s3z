import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Elysia, t } from "elysia";
import { BUCKET, client } from "./client";

export const routes = new Elysia()
  .post(
    "/upload",
    async ({ body }) => {
      const staging = await mkdtemp(join(tmpdir(), "s3z-"));
      try {
        const paths: string[] = [];
        const files = Array.isArray(body.files) ? body.files : [body.files];
        for (const file of files) {
          const dest = join(staging, file.name);
          await writeFile(dest, Buffer.from(await file.arrayBuffer()));
          paths.push(dest);
        }

        const results = await client.upload({
          sources: paths,
          bucket: BUCKET,
          prefix: body.prefix ?? "",
        });

        return results.map((r) => ({
          key: r.key,
          size: r.size,
          parts: r.parts,
          etag: r.etag,
        }));
      } finally {
        await rm(staging, { recursive: true, force: true });
      }
    },
    {
      detail: {
        summary: "Upload files",
        description:
          "Upload one or more files to S3. Large files are automatically split into concurrent multipart uploads.",
        tags: ["s3"],
      },
      body: t.Object({
        files: t.Files({ description: "Files to upload." }),
        prefix: t.Optional(
          t.String({
            description: "Key prefix prepended to each uploaded file name.",
          }),
        ),
      }),
      response: t.Array(
        t.Object({
          key: t.String({ description: "S3 object key." }),
          size: t.Number({ description: "File size in bytes." }),
          parts: t.Number({
            description: "Number of multipart parts used (1 = single PUT).",
          }),
          etag: t.String({ description: "ETag returned by S3." }),
        }),
      ),
    },
  )
  .get(
    "/download",
    async ({ query, set }) => {
      const staging = await mkdtemp(join(tmpdir(), "s3z-"));
      const slash = query.key.lastIndexOf("/");
      const prefix = slash >= 0 ? query.key.slice(0, slash + 1) : "";

      const results = await client.download({
        bucket: BUCKET,
        prefix,
        destDir: staging,
      });

      const match = results.find((r) => r.key === query.key);
      if (!match) {
        set.status = 404;
        return { detail: `key not found: ${query.key}` };
      }
      return Bun.file(match.dest);
    },
    {
      detail: {
        summary: "Download a file",
        description: "Download a single object from S3 by its full key.",
        tags: ["s3"],
      },
      query: t.Object({
        key: t.String({
          description: "Full S3 object key to download.",
        }),
      }),
    },
  )
  .get(
    "/list",
    async ({ query }) => {
      const objects = await client.list({
        bucket: BUCKET,
        prefix: query.prefix ?? "",
        delimiter: query.delimiter ?? undefined,
      });

      return objects.map((o) => ({
        key: o.key,
        size: o.size,
        etag: o.etag,
        lastModified: o.lastModified,
      }));
    },
    {
      detail: {
        summary: "List objects",
        description: "List objects under a prefix. Pass delimiter=/ for directory-style grouping.",
        tags: ["s3"],
      },
      query: t.Object({
        prefix: t.Optional(
          t.String({
            description: "Key prefix to filter by (e.g. data/2024/).",
          }),
        ),
        delimiter: t.Optional(
          t.String({
            description: "Delimiter for directory-style grouping (typically /).",
          }),
        ),
      }),
      response: t.Array(
        t.Object({
          key: t.String({ description: "Full object key." }),
          size: t.Number({ description: "Object size in bytes." }),
          etag: t.String({ description: "ETag of the object." }),
          lastModified: t.String({
            description: "Last-modified timestamp (ISO 8601).",
          }),
        }),
      ),
    },
  );
