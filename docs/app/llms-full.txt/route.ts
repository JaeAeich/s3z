import { source } from '@/lib/source';
import { getLLMText } from '@/lib/get-llm-text';

export const revalidate = false;

export async function GET() {
  const pages = source.getPages().map((page) => getLLMText(page));
  const texts = await Promise.all(pages);
  return new Response(texts.join('\n\n'));
}
