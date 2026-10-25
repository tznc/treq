import type React from 'react';
import DocItemLayout from '@theme-original/DocItem/Layout';
import type {Props} from '@theme/DocItem/Layout';
import Head from '@docusaurus/Head';
import {useDoc} from '@docusaurus/plugin-content-docs/client';
import DownloadCTA from '@site/src/components/DownloadCTA';

const SITE_URL = 'https://treq.dev';
const ORG = {'@type': 'Organization', name: 'Treq', url: SITE_URL};

function buildSchema(metadata: ReturnType<typeof useDoc>['metadata']) {
  const {title, description, permalink, lastUpdatedAt} = metadata;
  const url = `${SITE_URL}${permalink}`;
  const base = {
    '@context': 'https://schema.org',
    ...(description ? {description} : {}),
    url,
    ...(lastUpdatedAt
      ? {dateModified: new Date(lastUpdatedAt * 1000).toISOString()}
      : {}),
  };

  if (permalink.includes('/how-to/')) {
    return {'@type': 'HowTo', name: title, ...base, publisher: ORG};
  }

  if (permalink.startsWith('/learn/tutorials/')) {
    return {
      '@type': 'LearningResource',
      name: title,
      ...base,
      learningResourceType: 'Tutorial',
      educationalLevel: 'Beginner',
      provider: ORG,
    };
  }

  if (permalink.startsWith('/learn/')) {
    return {
      '@type': 'LearningResource',
      name: title,
      ...base,
      educationalLevel: 'Beginner',
      provider: ORG,
    };
  }

  return {'@type': 'TechArticle', headline: title, ...base, publisher: ORG};
}

export default function DocItemLayoutWrapper(props: Props): React.ReactElement {
  const {metadata} = useDoc();
  return (
    <>
      <Head>
        <script type="application/ld+json">
          {JSON.stringify(buildSchema(metadata))}
        </script>
      </Head>
      <DocItemLayout {...props}>
        {props.children}
        {/^\/(learn|compare)\//.test(metadata.permalink) ? <DownloadCTA /> : null}
      </DocItemLayout>
    </>
  );
}
