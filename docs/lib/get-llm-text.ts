import type { source } from '@/lib/source';

export async function getLLMText(
  page: NonNullable<ReturnType<typeof source.getPage>>,
) {
  const processed = await page.data.getText('processed');
  return `# ${page.data.title} (${page.url})\n\n${processed}`;
}
