import { S3Client } from "s3z";

const REGION = process.env.AWS_REGION ?? "us-east-1";
const ENDPOINT = process.env.S3_ENDPOINT;

export const BUCKET = process.env.S3_BUCKET ?? "bench-bucket";

export const client = await S3Client.create({
  region: REGION,
  accessKey: process.env.AWS_ACCESS_KEY_ID ?? undefined,
  secretKey: process.env.AWS_SECRET_ACCESS_KEY ?? undefined,
  endpoint: ENDPOINT ?? undefined,
});
