import type React from 'react';
import Head from '@docusaurus/Head';
import {useBreadcrumbsStructuredData} from '@docusaurus/plugin-content-docs/client';
import type {Props} from '@theme/DocBreadcrumbs/StructuredData';
import {withTrailingSlash} from '@site/src/lib/trailingSlash';

// Same as the theme's, except each item URL gets the trailing slash the site
// serves pages at; the theme uses the bare permalink, which redirects.
export default function DocBreadcrumbsStructuredData(props: Props): React.ReactElement {
  const data = useBreadcrumbsStructuredData({breadcrumbs: props.breadcrumbs});
  // The theme builds a plain array of ListItems; schema-dts types it loosely.
  const items: readonly unknown[] = Array.isArray(data.itemListElement) ? data.itemListElement : [];
  const itemListElement = items.map((entry) =>
    typeof entry === 'object' && entry !== null && 'item' in entry && typeof entry.item === 'string'
      ? {...entry, item: withTrailingSlash(entry.item)}
      : entry,
  );
  return (
    <Head>
      <script type="application/ld+json">
        {JSON.stringify({...data, itemListElement})}
      </script>
    </Head>
  );
}
