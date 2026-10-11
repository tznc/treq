// Plans on the pricing page (prds/billing-and-teams.md, "Plans"). Only list
// what the service enforces today. A feature that is gated but not open to
// everyone yet says "private alpha".

export const PRICING_SCHEMA = {
  '@context': 'https://schema.org',
  '@type': 'Product',
  name: 'Treq',
  description:
    'Open source desktop app for running coding agents in stacked workspaces. Free on your machine. Pro and Team add the Treq GitHub App, Linear OAuth, and a managed cloud workspace in private alpha.',
  url: 'https://treq.dev/pricing',
  brand: {
    '@type': 'Brand',
    name: 'Treq',
  },
  offers: [
    {
      '@type': 'Offer',
      name: 'Free',
      price: '0',
      priceCurrency: 'USD',
      description:
        'The full desktop app. GitHub through your local gh CLI and Linear through a personal API key, for any repository.',
    },
    {
      '@type': 'Offer',
      name: 'Pro',
      price: '15',
      priceCurrency: 'USD',
      unitText: 'user/month',
      description:
        'The Treq GitHub App and Linear OAuth, plus the merge queue and a managed cloud workspace in private alpha. Billed per user per month after a 14-day trial that needs a card.',
    },
    {
      '@type': 'Offer',
      name: 'Team',
      price: '199',
      priceCurrency: 'USD',
      unitText: 'month',
      description:
        'Everything in Pro for 5 members, with a shared GitHub App installation and a shared merge queue in private alpha. Each extra member is US$8 per month. Billed per month.',
    },
  ],
};

export const FAQ_ITEMS = [
  {
    question: 'What is free, and what do I pay for?',
    answer:
      "The desktop app is free and open source, and runs everything on your machine. GitHub works through your local gh CLI and Linear through a personal API key, for any repository. You pay for the features that run on Treq's servers: the Treq GitHub App, Linear OAuth, and two features in private alpha, the merge queue and a managed cloud workspace.",
  },
  {
    question: 'How does the Pro trial work?',
    answer:
      'Pro starts with a 14-day free trial. Checkout asks for a card, and Stripe charges it when the trial ends unless you cancel first. Each account gets one trial. If you cancel, Pro stays on until the end of the period you already have. Team has no trial.',
  },
  {
    question: 'How do Team seats count?',
    answer:
      'Team costs US$199 per month and includes 5 members, counting the owner. Each member beyond 5 adds US$8 per month, prorated from the day they join. Pending invites are not billed. An invite link works once and expires after 7 days. A member who leaves or is removed loses Pro at once, and their seat stops billing.',
  },
  {
    question: 'Is the desktop app still free on Pro?',
    answer:
      'Yes. Pro and Team are cloud subscriptions on top of the same open source desktop app. Your local workspaces stay on your machine either way.',
  },
  {
    question: 'How do I upgrade?',
    answer:
      'Create an account, then start Pro from the Subscription tab of your dashboard. For Team, create an organization on the Team tab, buy Team there, and send your teammates their invite links.',
  },
] as const;
