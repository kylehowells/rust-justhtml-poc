# rust-justhtml (Proof of Concept)

> **WARNING: This is a proof of concept and should NOT be used in production projects.**
>
> For production use, please use the battle-tested [servo/html5ever](https://github.com/servo/html5ever) library instead.

## Purpose

This project is a proof of concept that ports [justhtml](https://github.com/EmilStenstrom/justhtml) (a Python HTML5 parser) to Rust. The goal was to compare the performance of Rust parsing HTML **without** the custom performance optimizations that the [servo/html5ever](https://github.com/servo/html5ever) library implements.

### Why This Exists

The [justhtml](https://github.com/EmilStenstrom/justhtml) family of parsers exists in multiple languages:
- **Python**: [justhtml](https://github.com/EmilStenstrom/justhtml)
- **JavaScript**: [justjshtml](https://github.com/simonw/justjshtml)
- **Swift**: [swift-justhtml](https://github.com/kylehowells/swift-justhtml)
- **Rust (this repo)**: rust-justhtml (proof of concept)

When comparing these implementations against [servo/html5ever](https://github.com/servo/html5ever), html5ever was significantly faster. This raised the question:

**How much of html5ever's performance advantage comes from Rust itself vs. its custom performance optimizations?**

html5ever uses several sophisticated optimizations documented in [html5ever-architecture.md](https://github.com/kylehowells/swift-justhtml/blob/master/notes/html5ever-architecture.md), including:
- Tendril-based string handling (avoiding allocations)
- Arena allocation for DOM nodes
- Zero-copy parsing where possible
- Pre-interned atoms for common HTML tags/attributes
- Optimized state machine code generation

By porting the simpler justhtml implementation to Rust (without these optimizations), we can isolate how much performance comes from Rust's inherent speed vs. html5ever's architectural decisions.

## Results Summary

Full benchmark details: [BENCHMARK_RESULTS.md](./BENCHMARK_RESULTS.md) | [MEMORY_RESULTS.md](./MEMORY_RESULTS.md)

### Performance (Parse Time)

| Implementation | Total Time | vs html5ever |
|----------------|------------|--------------|
| **html5ever** (Rust) | 302ms | 1.0x (baseline) |
| **rust-justhtml** (Rust) | 696ms | 2.3x slower |
| **justjshtml** (JavaScript) | 1,206ms | 4.0x slower |
| **swift-justhtml** (Swift) | 1,319ms | 4.4x slower |
| **justhtml** (Python) | 4,197ms | 13.9x slower |

### Memory Usage (Peak RSS)

| Implementation | Average Memory | vs html5ever |
|----------------|----------------|--------------|
| **html5ever** (Rust) | 42 MB | 1.0x (baseline) |
| **swift-justhtml** (Swift) | 103 MB | 2.5x more |
| **justhtml** (Python) | 106 MB | 2.5x more |
| **rust-justhtml** (Rust) | 149 MB | 3.5x more |
| **justjshtml** (JavaScript) | 226 MB | 5.4x more |

### Key Findings

1. **Rust itself provides ~2x speedup**: rust-justhtml is about 1.9x faster than Swift and 6x faster than Python, despite using the same simple parsing approach.

2. **html5ever's optimizations provide another ~2.3x speedup**: html5ever is 2.3x faster than rust-justhtml, showing the significant impact of its custom optimizations (tendrils, arena allocation, pre-interned atoms, etc.).

3. **Memory is a different story**: rust-justhtml actually uses more memory than Swift and Python implementations. This is because it uses simple `String` and `Vec` types without html5ever's memory-optimized data structures. html5ever's tendril and arena-based approach is crucial for memory efficiency.

4. **Total html5ever advantage**: Combining Rust's inherent speed with html5ever's optimizations results in a ~4.4x speedup over Swift and ~14x over Python.

## Related Projects

- [servo/html5ever](https://github.com/servo/html5ever) - Production-ready Rust HTML5 parser (use this!)
- [swift-justhtml](https://github.com/kylehowells/swift-justhtml) - Swift implementation
- [justhtml](https://github.com/EmilStenstrom/justhtml) - Original Python implementation
- [justjshtml](https://github.com/simonw/justjshtml) - JavaScript implementation

## Test Compliance

This implementation passes all 1,735 html5lib tree construction tests, ensuring spec-compliant HTML5 parsing.

## License

MIT
