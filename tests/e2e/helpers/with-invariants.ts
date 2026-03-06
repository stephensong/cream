import { test as base } from '@playwright/test';
import { checkInvariants } from './check-invariants';

/**
 * Extended Playwright test that runs CURD conservation invariant checks
 * before and after each test. Use this instead of `test` from '@playwright/test'
 * in any test file that modifies wallet balances or ledger state.
 *
 * Usage:
 *   import { test, expect } from '../helpers/with-invariants';
 */
export const test = base.extend({});

test.beforeEach(async ({}, testInfo) => {
  checkInvariants(`e2e ${testInfo.titlePath.join(' > ')} [pre]`);
});

test.afterEach(async ({}, testInfo) => {
  checkInvariants(`e2e ${testInfo.titlePath.join(' > ')} [post]`);
});

export { expect } from '@playwright/test';
