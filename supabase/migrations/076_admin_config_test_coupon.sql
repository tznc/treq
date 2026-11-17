-- The internal test accounts and their checkout coupon move from the
-- TREQ_TEST_ACCOUNT_EMAIL / TREQ_TEST_COUPON_ID env vars into admin_config.
-- billing-checkout applies coupon_id when the caller's email equals one of
-- emails exactly; an empty list or coupon_id turns it off.
insert into treq_internal.admin_config_schemas (key, schema) values
  ('test_coupon', '{
    "type": "object",
    "required": ["emails", "coupon_id"],
    "properties": {
      "emails": {"type": "array", "items": {"type": "string"}},
      "coupon_id": {"type": "string"}
    }
  }');

insert into treq_internal.admin_config (key, value, description) values
  ('test_coupon', '{"emails": [], "coupon_id": ""}',
   'Internal test account emails and the Stripe coupon applied to their checkouts (100% off). An empty list or coupon_id disables it.');
