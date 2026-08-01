# ⚛️ The Hadron Physics Metaphor

Hadron uses particle physics as a cohesive mental model for multi-agent operating system concepts. This provides a clear, unifying vocabulary across the protocol, user interface, daemon logs, and system events.

---

## Why Physics?

In quantum physics, elementary particles interact over quantum fields, bound together by gauge bosons to form complex composite structures. 

Similarly, in Hadron:
- Individual AI agents (**Quarks**) are elementary units of intelligence.
- Agents interact over an append-only event bus (**Field**).
- Background daemons (**Gluons**) bind agents together and route interactions.
- The desktop interface (**Chamber**) acts like a bubble chamber, rendering particle tracks and interactions visible to human operators.

---

## Metaphor Mapping

| Concept | Hadron Component | Physics Parallel |
| :--- | :--- | :--- |
| **Swarm OS** | `Hadron` | Composite particle formed by bound quarks |
| **Agent Seat** | `Quark` | Fundamental particle carrying flavor and charge |
| **Instructions** | `Preon` | Substructure loaded into a quark |
| **Event Bus** | `Field` (`field.jsonl`) | Quantum field conveying interactions |
| **Daemon** | `Gluon` (`hadron-gluon`) | Gauge boson mediating quark bindings |
| **Protocol Schema** | `Lattice` (`hadron-lattice`) | Spacetime lattice structure |
| **Desktop UI** | `Chamber` (`hadron-chamber`) | Cloud / bubble detector chamber |
| **Knowledge Base** | `Nucleus` | Dense central atomic core |
| **Role / Duty** | `Flavor` (Orchestrator, Worker) | Quark flavors (up, down, charm...) |
| **Token Telemetry** | `Energy` & `Ledger` | Conservation of energy & energy states |
| **Turn Status** | `Excited` vs `Ground` state | Energy excitation levels |
| **Invariants** | `Standard Model` | Fundamental physical laws |
