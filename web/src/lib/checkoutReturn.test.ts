import {checkoutReturnPlan} from './checkoutReturn';

test('a Pro checkout returns to the subscription tab', () => {
  expect(checkoutReturnPlan('?tab=subscription&session_id=cs_1')).toBe('pro');
});

test('a Team checkout returns to the team tab', () => {
  expect(checkoutReturnPlan('?tab=team&session_id=cs_1')).toBe('team');
});

test('no session_id means no checkout return', () => {
  expect(checkoutReturnPlan('?tab=team')).toBeNull();
  expect(checkoutReturnPlan('')).toBeNull();
});

test('a session_id without a known tab is not attributed to either plan', () => {
  expect(checkoutReturnPlan('?session_id=cs_1')).toBeNull();
  expect(checkoutReturnPlan('?tab=integrations&session_id=cs_1')).toBeNull();
});
