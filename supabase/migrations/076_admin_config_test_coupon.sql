-- The internal test account and its checkout coupon move from the
-- TREQ_TEST_ACCOUNT_EMAIL / TREQ_TEST_COUPON_ID env vars into admin_config.
-- billing-checkout applies coupon_id when the caller's email equals email
-- exactly; empty strings turn it off.
insert into treq_internal.admin_config_schemas (key, schema) values
  ('test_coupon', '{
    "type": "object",
    "required": ["email", "coupon_id"],
    "properties": {
      "email": {"type": "string"},
      "coupon_id": {"type": "string"}
    }
  }');

insert into treq_internal.admin_config (key, value, description) values
  ('test_coupon', '{"email": "", "coupon_id": ""}',
   'Internal test account email and the Stripe coupon applied to its checkouts (100% off). Empty strings disable it.');
