//! List subcommand implementation.

use s3z::{ListRequest, S3Client};

use crate::{cli::ListArgs, fmt};

/// Execute the ls subcommand.
pub(crate) async fn run(client: &S3Client, args: &ListArgs, quiet: bool) -> anyhow::Result<()> {
    let req = ListRequest::new(&args.bucket, &args.prefix);
    let mut paginator = client.list(req);

    let mut total_objects: u64 = 0;
    let mut total_size: u64 = 0;

    while let Some(page) = paginator.next_page().await? {
        for obj in &page.objects {
            #[expect(clippy::arithmetic_side_effects, reason = "display counters")]
            {
                total_objects += 1;
                total_size += obj.size;
            }
            if !quiet {
                let date = obj.last_modified.get(..10).unwrap_or(&obj.last_modified);
                println!("{:>10}  {}  {}", fmt::bytes(obj.size), date, obj.key);
            }
        }
    }

    if !quiet {
        println!();
        println!("{} object(s), {} total", total_objects, fmt::bytes(total_size),);
    }

    Ok(())
}
