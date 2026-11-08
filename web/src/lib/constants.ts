// Stripe publishable keys for embedded Checkout on the dashboard. The test
// key belongs to Treq's Stripe test mode (TREQ-162); production builds use
// the live key (TREQ-326).
const STRIPE_PUBLISHABLE_KEY_TEST =
  "pk_test_51Sz674Gdctkyk7T0pwBiRMnVzSgXF3kqpChpkC16Ixs3FWmx0s1j5GXudB5CcXrFU3v5ittemjWRNS8XEUG8O0fu000KLIu586";
export const STRIPE_PUBLISHABLE_KEY_PROD =
  "pk_live_51Sz66vGs2Vsp10hMAZYcyl899Odh44rgwOK86KuAlH5EIvy22UCp3N70Bqc0vfgb0IjwNovWH8NI4lMK6M97kncu00mCjm2JPQ";

export const STRIPE_PUBLISHABLE_KEY =
  process.env.NODE_ENV === "production"
    ? STRIPE_PUBLISHABLE_KEY_PROD
    : STRIPE_PUBLISHABLE_KEY_TEST;

export const APP_DEEP_LINK = "treq://";

// GitHub App — update with your actual App slug after creating it at
// https://github.com/settings/apps/new
export const GITHUB_APP_NAME =
  process.env.NODE_ENV === "production" ? "treq" : "treq-dev";

export const GITHUB_APP_INSTALL_URL = `https://github.com/apps/${GITHUB_APP_NAME}/installations/new`;
