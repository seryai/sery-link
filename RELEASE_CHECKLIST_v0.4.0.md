# Release Checklist: v0.4.0 - Three-Tier Strategy

**Target Release Date:** TBD
**Release Manager:** TBD
**Status:** 🟡 In Progress

---

## Pre-Release: Code Complete ✅

### Code Implementation
- [x] ✅ Backend implementation complete (auth.rs, config.rs, commands.rs)
- [x] ✅ Frontend implementation complete (6 files modified/created)
- [x] ✅ Unit tests written (19 new tests)
- [x] ✅ All new tests passing (19/19)
- [x] ✅ Code compiles without errors (Rust + TypeScript)
- [x] ✅ No breaking changes introduced

### Documentation
- [x] ✅ TESTING_v0.4.0.md created (750+ lines)
- [x] ✅ IMPLEMENTATION_REFERENCE.md created (600+ lines)
- [x] ✅ DEVELOPER_QUICKSTART.md created (400+ lines)
- [x] ✅ CHANGELOG.md updated
- [x] ✅ IMPLEMENTATION_COMPLETE.md created
- [x] ✅ DELIVERABLES_SUMMARY.md created

### Developer Tools
- [x] ✅ reset-dev.sh script created
- [x] ✅ run-tests.sh script created
- [x] ✅ check-config.sh script created
- [x] ✅ All scripts tested and working

---

## Phase 1: Internal Testing (2-3 days)

### Setup
- [ ] Create dedicated testing environment
- [ ] Document test setup instructions
- [ ] Assign testing team members
- [ ] Set up error tracking/reporting

### Manual Testing (from TESTING_v0.4.0.md)
- [ ] **Test 1:** Fresh install flow (< 60 sec target)
  - [ ] Install app from scratch
  - [ ] Select Local Vault mode
  - [ ] Add folder successfully
  - [ ] Execute FREE recipe
  - [ ] Verify time < 60 seconds

- [ ] **Test 2:** Tier gating enforcement
  - [ ] Verify only 5 FREE recipes visible in LocalOnly
  - [ ] Verify upgrade banner displays
  - [ ] Verify PRO recipes blocked
  - [ ] Verify error messages are clear

- [ ] **Test 3:** Existing user migration
  - [ ] Install v0.3.x and authenticate
  - [ ] Upgrade to v0.4.0
  - [ ] Verify auto-migration to WorkspaceKey
  - [ ] Verify no data loss
  - [ ] Verify all features work

- [ ] **Test 4:** BYOK mode
  - [ ] Start in LocalOnly mode
  - [ ] Enter API key
  - [ ] Verify PRO features unlock
  - [ ] Test PRO recipe execution

- [ ] **Test 5:** Mode switching
  - [ ] LocalOnly → WorkspaceKey
  - [ ] BYOK → WorkspaceKey
  - [ ] Verify no errors

- [ ] **Test 6:** Feature matrix validation
  - [ ] Run automated feature test script
  - [ ] Verify all gates work correctly

- [ ] **Test 7:** UI/UX validation
  - [ ] Onboarding wizard flows smoothly
  - [ ] Mode selection cards are clear
  - [ ] Upgrade prompts are attractive
  - [ ] No visual glitches

- [ ] **Test 8:** Error handling
  - [ ] Invalid API keys handled gracefully
  - [ ] Tier violations show correct prompts
  - [ ] Network errors handled

- [ ] **Test 9:** Performance benchmarks
  - [ ] Startup time < 2 sec (LocalOnly)
  - [ ] Recipe loading < 500ms
  - [ ] No regressions vs v0.3.x

- [ ] **Test 10:** Security validation
  - [ ] LocalOnly mode makes no network calls
  - [ ] Tokens encrypted in keyring
  - [ ] No sensitive data in exports

### Automated Testing
- [ ] Run `./scripts/run-tests.sh`
- [ ] All auth tests pass (9/9)
- [ ] All config tests pass (10/10)
- [ ] No new test failures introduced

### Bug Triage
- [ ] Document all bugs found
- [ ] Categorize: Critical / High / Medium / Low
- [ ] Fix critical bugs before beta
- [ ] Create tickets for non-critical bugs

### Performance Testing
- [ ] Measure startup time across modes
- [ ] Measure recipe loading time
- [ ] Memory usage profiling
- [ ] Compare with v0.3.x baseline

---

## Phase 2: Pre-Beta Preparation (1 day)

### Version Bump
- [ ] Update `package.json` → `"version": "0.4.0"`
- [ ] Update `src-tauri/Cargo.toml` → `version = "0.4.0"`
- [ ] Update `src-tauri/tauri.conf.json` → `"version": "0.4.0"`

### Build Verification
- [ ] Clean build: `cargo clean && cargo build --release`
- [ ] Frontend build: `npm run build`
- [ ] Full app build: `npm run tauri build`
- [ ] Verify .dmg/.app created successfully
- [ ] Test installation on clean macOS

### Code Review
- [ ] Self-review all changes
- [ ] Peer review by 2+ team members
- [ ] Address all review comments
- [ ] Final approval from tech lead

### Documentation Review
- [ ] Verify all docs are up to date
- [ ] Check for broken links
- [ ] Verify code examples work
- [ ] Proofread for typos

### Marketing Prep
- [ ] Draft release announcement
- [ ] Prepare feature highlights
- [ ] Create demo video/screenshots
- [ ] Update website documentation

---

## Phase 3: Beta Release (5-7 days)

### Beta Distribution
- [ ] Select 50-100 beta testers
- [ ] Prepare beta testing instructions
- [ ] Set up feedback collection method
- [ ] Distribute beta builds

### Beta Testing Goals
- [ ] Validate fresh install flow
- [ ] Measure actual time-to-first-query
- [ ] Track mode adoption (LocalOnly vs Workspace)
- [ ] Collect upgrade conversion data
- [ ] Monitor error rates

### Monitoring
- [ ] Set up error tracking
- [ ] Monitor mode distribution metrics
- [ ] Track feature usage
- [ ] Collect user feedback

### Beta Bug Fixes
- [ ] Triage beta feedback daily
- [ ] Fix critical issues immediately
- [ ] Document known issues
- [ ] Prepare beta → production fixes

### Success Criteria
- [ ] < 5 critical bugs reported
- [ ] 80%+ of users complete onboarding
- [ ] Average time-to-first-query < 60 sec
- [ ] Positive feedback from beta testers

---

## Phase 4: Production Release Preparation (2-3 days)

### Final Testing
- [ ] Retest all critical paths
- [ ] Verify beta fixes work
- [ ] Run full regression suite
- [ ] Test upgrade path one final time

### Build Production Release
- [ ] Final version bump (if needed)
- [ ] Build production .dmg
- [ ] Sign application (macOS notarization)
- [ ] Test signed build

### Release Notes
- [ ] Finalize CHANGELOG.md
- [ ] Write user-facing release notes
- [ ] Prepare migration guide
- [ ] Create upgrade FAQ

### Infrastructure
- [ ] Verify backend ready for increased load
- [ ] Check CDN configuration
- [ ] Prepare rollback plan
- [ ] Set up production monitoring

### Communication Plan
- [ ] Email to existing users
- [ ] Blog post announcement
- [ ] Social media posts
- [ ] Update documentation site

---

## Phase 5: Production Release (Release Day)

### Pre-Launch (Morning)
- [ ] Final build verification
- [ ] Team briefing on launch plan
- [ ] Support team ready
- [ ] Monitoring dashboards ready

### Launch Sequence
- [ ] 🚀 Upload .dmg to distribution server
- [ ] Update download links
- [ ] Publish release on GitHub
- [ ] Send announcement email
- [ ] Post on social media
- [ ] Update website

### Immediate Post-Launch (First 2 hours)
- [ ] Monitor error rates
- [ ] Watch for crash reports
- [ ] Monitor support channels
- [ ] Track download metrics
- [ ] Verify auto-update works

### First Day Monitoring
- [ ] Check mode adoption rates
- [ ] Monitor upgrade conversions
- [ ] Track time-to-first-query
- [ ] Review support tickets
- [ ] Address urgent issues

---

## Phase 6: Post-Launch (Week 1)

### Metrics Review (Daily)
- [ ] User adoption rate
- [ ] Mode distribution (LocalOnly/BYOK/Workspace)
- [ ] Upgrade conversion rate
- [ ] Average session duration
- [ ] Feature usage patterns

### Support
- [ ] Monitor support tickets
- [ ] Create FAQ based on common questions
- [ ] Update documentation as needed
- [ ] Respond to user feedback

### Bug Fixes
- [ ] Triage reported issues
- [ ] Release hotfix if critical bugs found
- [ ] Update known issues list

### Success Metrics Validation
- [ ] Time to first query: Target < 60 sec
- [ ] LocalOnly adoption: Target 40%
- [ ] Upgrade conversion: Target 20% in 30 days
- [ ] User retention: Target 70% weekly active

---

## Phase 7: Post-Launch (Week 2-4)

### Data Analysis
- [ ] Analyze mode adoption trends
- [ ] Review upgrade conversion funnel
- [ ] Identify friction points
- [ ] Plan optimizations

### Documentation Updates
- [ ] Update based on user feedback
- [ ] Add common troubleshooting steps
- [ ] Create video tutorials
- [ ] Improve onboarding docs

### Planning v0.5.0
- [ ] Review v0.4.0 learnings
- [ ] Prioritize BYOK features
- [ ] Plan next iteration
- [ ] Update roadmap

---

## Rollback Plan (If Needed)

### Triggers for Rollback
- Critical security vulnerability
- Data loss issues
- > 10% crash rate
- Migration failures affecting > 5% of users

### Rollback Steps
1. [ ] Revert download links to v0.3.x
2. [ ] Send notification to affected users
3. [ ] Disable auto-update temporarily
4. [ ] Investigate root cause
5. [ ] Prepare hotfix
6. [ ] Plan re-release

---

## Sign-Off

### Internal Approval
- [ ] Engineering Lead: _______________ Date: _______
- [ ] QA Lead: _______________ Date: _______
- [ ] Product Manager: _______________ Date: _______
- [ ] CEO/CTO: _______________ Date: _______

### Release Authorization
- [ ] Beta Release Approved: _______________ Date: _______
- [ ] Production Release Approved: _______________ Date: _______

---

## Notes

### Decisions Made
-
-

### Issues Encountered
-
-

### Lessons Learned
-
-

---

**Last Updated:** 2026-04-15
**Next Review:** Before Beta Release
