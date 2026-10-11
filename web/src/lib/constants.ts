// Stripe publishable keys for embedded Checkout on the dashboard. The test
// key belongs to Treq's Stripe test mode (TREQ-162). The live key is left
// empty for the owner to fill in when live payments launch (TREQ-326).
// While it is empty, production builds never open Checkout.
const STRIPE_PUBLISHABLE_KEY_TEST =
  "pk_test_51Sz674Gdctkyk7T0pwBiRMnVzSgXF3kqpChpkC16Ixs3FWmx0s1j5GXudB5CcXrFU3v5ittemjWRNS8XEUG8O0fu000KLIu586";
export const STRIPE_PUBLISHABLE_KEY_PROD = "";

export const STRIPE_PUBLISHABLE_KEY =
  process.env.NODE_ENV === "production"
    ? STRIPE_PUBLISHABLE_KEY_PROD
    : STRIPE_PUBLISHABLE_KEY_TEST;

export const APP_DEEP_LINK = "treq://";

// GitHub App — update with your actual App slug after creating it at
// https://github.com/settings/apps/new
export const GITHUB_APP_NAME =
  process.env.NODE_ENV === "production" ? "treq-merge-queue" : "treq-merge-queue-dev";

export const GITHUB_APP_INSTALL_URL = `https://github.com/apps/${GITHUB_APP_NAME}/installations/new`;
