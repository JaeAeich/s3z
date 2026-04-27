import { source } from '@/lib/source';
import { DocsLayout } from 'fumadocs-ui/layouts/docs';
import type { ReactNode } from 'react';

export default function Layout({
  children,
}: {
  children: ReactNode;
}): ReactNode {
  return (
    <DocsLayout
      tree={source.pageTree}
      nav={{
        title: (
          <span className="font-mono text-lg font-extrabold italic tracking-tight">
            s<span className="text-[#c8512a]">3</span>z
          </span>
        ),
      }}
      sidebar={{
        footer: (
          <div className="px-2 py-3 text-xs text-fd-muted-foreground">
            Built by{' '}
            <a
              href="https://github.com/JaeAeich"
              className="text-[#c8512a] hover:underline"
            >
              @JaeAeich
            </a>
          </div>
        ),
      }}
    >
      {children}
    </DocsLayout>
  );
}
