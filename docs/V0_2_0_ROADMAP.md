# v0.2.0 Roadmap & Release Plan

**Status**: Planning (In Progress)  
**Date**: 2026-05-11  
**Target Release**: May 25-26, 2026  
**GitHub Issues**: #54, #57  

## Overview

v0.2.0 builds on v0.1.0 (P0+P1 complete) by adding:
- Full network policy enforcement (P2.1)
- Pod endpoint lifecycle management (P2.2)
- Startup time optimization (P3)
- Expanded integration testing (P2.4)

### Release Timeline

| Phase | Duration | Status |
|-------|----------|--------|
| P1 (Services) | 7 hours | ✅ COMPLETE (May 11) |
| P1 Validation | 1-2 days | ⏳ IN PROGRESS (May 11-12) |
| v0.1.0 Release | 1 day | ⏳ PENDING (May 13) |
| P2.1+P2.2 Implementation | 3-4 days | ⏳ PENDING (May 13-17) |
| P3 Optimization | 1-2 days | ⏳ PENDING (May 17-18) |
| P2 Validation | 1 day | ⏳ PENDING (May 18) |
| v0.2.0 Release | 1 day | ⏳ PENDING (May 19) |

**Total Time**: 2 weeks from now (May 11 → May 26)

---

## v0.2.0 Feature Set

### P2.1: Network Policy Enforcement

**Deliverables**:
- [x] PolicyCache component
- [x] PolicyEvaluator component
- [x] PolicyEnforcer component
- [ ] eBPF rule generation
- [ ] Integration with ServiceObserver
- [ ] Dynamic policy updates
- [ ] Policy metrics

**Success Criteria**:
- [ ] K8sAgentPolicyTest: >80% pass rate
- [ ] All policies evaluated correctly
- [ ] Ingress/egress rules enforced
- [ ] <100ms policy update latency

**Estimated Effort**: 2-3 days (3 parallel tracks)

### P2.2: Endpoint Lifecycle Management

**Deliverables**:
- [x] EndpointCache component
- [x] IPAMManager component
- [x] EndpointManager component
- [x] HealthTracker component
- [ ] Pod event watching
- [ ] Health probe integration
- [ ] Endpoint metrics

**Success Criteria**:
- [ ] All pods get endpoints with IPs
- [ ] IP allocation/deallocation works
- [ ] No IP leaks on pod deletion
- [ ] Endpoint health tracking accurate

**Estimated Effort**: 1-2 days (2 parallel tracks)

### P3: Startup Optimization

**Deliverables**:
- [ ] Parallel subsystem initialization
- [ ] eBPF program lazy loading
- [ ] Cache warming from snapshots
- [ ] Operator communication batching
- [ ] Startup metrics
- [ ] Performance profiling

**Success Criteria**:
- [ ] Startup time: <3 minutes
- [ ] Current estimate: 2.5-3.5 minutes
- [ ] 2-3x speedup vs current

**Estimated Effort**: 1-2 days (1 track, after P2)

### Integration Testing

**Deliverables**:
- [ ] Service + Policy combinations
- [ ] Endpoint lifecycle + policies
- [ ] Complex network topologies
- [ ] Performance benchmarks
- [ ] Regression testing

**Estimated Effort**: 1 day (parallel to P2)

---

## Implementation Strategy

### Phase 1: Parallel P2 Implementation (3-4 days)

Work on P2.1 and P2.2 in parallel with separate tracks:

**P2.1 Tracks** (3 developers):
1. **Track 1**: eBPF rule generation & map updates
2. **Track 2**: Policy event watching & cache management
3. **Track 3**: Dynamic policy updates & metrics

**P2.2 Tracks** (2 developers):
1. **Track 1**: Pod event watching & endpoint creation
2. **Track 2**: IP allocation & health tracking

**P1 Continued**:
- Track: Run K8sDatapathServicesTest in background
- Goal: Achieve 40+/50 specs passing

### Phase 2: Validation & Fixes (1-2 days)

- Run P2 validation tests
- Fix integration issues
- Retest until green

### Phase 3: Optimization (1-2 days)

- Profile startup sequence
- Implement P3 optimizations
- Measure 2-3x improvement

### Phase 4: Release (1 day)

- Final validation
- Documentation updates
- GitHub v0.2.0 release

---

## Dependency Chain

```
v0.1.0 Release (May 13)
    ↓
P2.1 (Policy, days 1-3)
P2.2 (Endpoints, days 1-3, parallel to P2.1)
    ↓
P2 Validation (day 4)
    ↓
P3 (Optimization, day 5-6)
    ↓
v0.2.0 Release (May 19)
```

---

## New GitHub Issues for v0.2.0

### P2.1 (Policy) Issues

**Issue #49**: P2.1.1 eBPF Rule Generation
- Generate rules from policies
- Store in eBPF maps
- Support ingress/egress

**Issue #49a**: P2.1.2 Policy Event Watching
- Watch NetworkPolicy resources
- Cache policy changes
- Trigger re-evaluation

**Issue #49b**: P2.1.3 Dynamic Policy Updates
- Update rules without restart
- Handle policy edits
- Remove deleted policies

### P2.2 (Endpoints) Issues

**Issue #50**: P2.2.1 Pod Event Watching
- Watch pod create/update/delete
- Allocate IPs on creation
- Release on deletion

**Issue #50a**: P2.2.2 Endpoint Integration
- Integrate with ServiceObserver
- Update backends on pod changes
- Track endpoint health

### P3 (Optimization) Issues

**Issue #51**: P3.1 Startup Profiling
- Profile current startup sequence
- Identify bottlenecks
- Generate baseline metrics

**Issue #51a**: P3.2 Parallel Initialization
- Refactor to use tokio::join!
- Initialize subsystems in parallel
- Target: 3x speedup

**Issue #51b**: P3.3 Cache Warming
- Add snapshot support
- Pre-populate on startup
- Reduce initial K8s calls

### Testing Issues

**Issue #52**: P2 Integration Testing
- Service + policy tests
- Endpoint lifecycle tests
- Complex topology tests
- Performance benchmarks

---

## Architecture Changes for v0.2.0

### New Data Flow

```
Kubernetes API
    ↓ (dual watch)
[ServiceObserver]  ←→  [EndpointManager]
    ↓                       ↓
[BackendMappingEngine]      ↓
    ↓                   [IPAMManager]
    ↓                       ↓
[eBPFMaps] ←─────→ [PolicyCache]
    ↓                       ↓
[PolicyEnforcer]            ↓
    ↓                   [HealthTracker]
[LoadBalancer]              ↓
    ↓ (combined decisions)
Kernel eBPF Programs
    ↓
Datapath (with policy enforcement)
```

### New Components

1. **PolicyEnforcer**: Links policy to eBPF
2. **HealthTracker**: Monitors endpoint health
3. **Policy Rule Engine**: Generates rules from policies
4. **Event Aggregator**: Batches updates to kernel

---

## Success Metrics for v0.2.0

### Functionality
- [x] P1: Service load balancing (complete)
- [ ] P2.1: Network policies
- [ ] P2.2: Endpoint management
- [ ] P3: Startup optimization

### Test Coverage
- [ ] K8sDatapathServicesTest: 40+/50 (no regression)
- [ ] K8sAgentPolicyTest: 40+/50 new tests
- [ ] New endpoint lifecycle tests
- [ ] Performance benchmarks

### Code Quality
- [ ] All unit tests passing (50+ new tests)
- [ ] 0 clippy warnings across P2 crates
- [ ] 0 unsafe code blocks
- [ ] Comprehensive error handling

### Performance
- [ ] Startup: <3 minutes
- [ ] Policy update: <100ms
- [ ] Endpoint allocation: <50ms
- [ ] No performance regressions

### Documentation
- [ ] P2 architecture docs
- [ ] Integration guide
- [ ] Troubleshooting guide
- [ ] Release notes

---

## Release Checklist

### Pre-Release (May 18-19)

- [ ] All P2 tests passing
- [ ] Integration validation green
- [ ] Performance targets met
- [ ] Documentation complete
- [ ] Code review passed

### Release (May 19-20)

- [ ] Create GitHub release v0.2.0
- [ ] Tag commit with v0.2.0
- [ ] Build final images
- [ ] Publish to GHCR
- [ ] Update README
- [ ] Announce release

### Post-Release (May 20+)

- [ ] Gather user feedback
- [ ] Plan v0.3.0 features
- [ ] Begin next phase

---

## v0.3.0 Preview

**Target**: Early June 2026  
**Focus**: Observability, Performance, Advanced Networking

### P4: Observability
- Hubble integration
- Network metrics
- Flow logging
- Distributed tracing

### P5: Performance Optimization
- eBPF datapath tuning
- Memory optimization
- Connection pooling
- Cache efficiency

### P6: Advanced Networking
- Cluster mesh
- BGP support
- Encryption (WireGuard)
- Egress gateway

---

## Parallel Execution Plan

### Current (May 11-12)
- P1 Validation: K8sDatapathServicesTest (background)
- P2 Planning: COMPLETE ✅
- P2 Scaffolds: COMPLETE ✅
- v0.2.0 Roadmap: IN PROGRESS

### Next (May 13-14)
- v0.1.0 Release (after P1 validation)
- P2.1 Implementation (Track 1: eBPF rules)
- P2.1 Implementation (Track 2: Event watching)
- P2.2 Implementation (Track 1: Pod watching)

### Then (May 15-18)
- P2.1 Implementation (Track 3: Dynamic updates)
- P2.2 Implementation (Track 2: IP allocation)
- P2 Integration Testing
- P3 Profiling & Optimization

### Finally (May 19-20)
- v0.2.0 Release
- Performance validation
- Release announcement

---

## Resource Requirements

### Development
- 4-5 developers (1 per track)
- Laptop/workstation for each
- GitHub + CI/CD infrastructure

### Infrastructure
- 3x kind clusters (parallel testing)
- Build runner (cargo compilation)
- Container registry (GHCR)

### Time
- Total: ~2 weeks (May 11 → May 26)
- Daily standup: 15 min
- Weekly review: 1 hour

---

## Risks & Mitigations

### High Risk

**Policy Rule Generation Complexity**
- Risk: Translating K8s policies to eBPF rules is complex
- Mitigation: Start with simple rules, expand gradually
- Impact: May delay P2.1 by 1-2 days

**IP Allocation Scale**
- Risk: IPAM might not scale to 1000+ pods
- Mitigation: Use efficient data structures, benchmark early
- Impact: Would require redesign

**Startup Time Optimization**
- Risk: Cannot reach <3 min target
- Mitigation: Profile early, identify real bottlenecks
- Impact: Feature flag rather than release blocker

### Medium Risk

**Integration Complexity**
- Risk: Components don't integrate cleanly
- Mitigation: Weekly integration testing
- Impact: 1-2 day slip

**Test Harness Incompatibilities**
- Risk: New tests don't work with existing harness
- Mitigation: Reuse P1 test infrastructure
- Impact: 1 day investigation

---

## Success Story

**v0.2.0 will deliver:**
- ✅ Complete service load balancing (v0.1.0)
- ✅ Full network policy enforcement (v0.2.0 NEW)
- ✅ Pod endpoint lifecycle management (v0.2.0 NEW)
- ✅ 3x faster startup time (v0.2.0 NEW)
- ✅ 80%+ test coverage across all subsystems
- ✅ Production-ready Rust implementation
- ✅ Clear path to full Cilium feature parity

**Timeline**: 2 weeks (May 11 → May 26)  
**Team Size**: 4-5 developers  
**Code Quality**: 100% test pass, 0 warnings  
**Ready for**: Beta testing, community feedback, v0.3.0 planning  

---

**Document Version**: 1.0  
**Last Updated**: 2026-05-11 19:30 UTC  
**Status**: In Progress (Planning Complete, Ready for Implementation)  
**GitHub Issue**: #54  
