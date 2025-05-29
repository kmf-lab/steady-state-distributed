# Steady State Distributed

A distributed actor system demonstrating enterprise-grade communication patterns and network-based processing with the `steady_state` framework.

## 🌐 Distributed Overview

This project showcases enterprise-grade distributed processing patterns:

- **Multi-Process Architecture**: Split processing across independent publisher and subscriber pods
- **High-Performance Networking**: Aeron-based communication for ultra-low latency message transport
- **Serialization Pipeline**: Automatic conversion between typed data and network-efficient byte streams
- **Coordinated Shutdown**: Synchronized termination across distributed components
- **Stream Multiplexing**: Multiple data channels over single network connections
- **Transport Flexibility**: Support for both UDP and IPC communication protocols

## 🎯 System Architecture

**Distributed Processing Pipeline**:

**Publisher Pod**:
Generator → Serialize → Network Transport
↗ Heartbeat ↗

**Subscriber Pod**:
Network Transport → Deserialize → Worker → Logger
↗ (Split streams)

- **Publisher Generator**: Creates sequential data streams with coordinated timing
- **Publisher Heartbeat**: Provides synchronized timing signals across the network
- **Serialize Actor**: Converts typed data into network-optimized byte streams
- **Network Transport**: High-performance Aeron messaging for ultra-low latency communication
- **Deserialize Actor**: Reconstructs typed data from incoming byte streams
- **Subscriber Worker**: Processes distributed FizzBuzz logic with network coordination
- **Subscriber Logger**: Outputs results from distributed processing pipeline

## 🧠 Distributed Concepts

### Local vs Distributed Processing

| Local Processing | Distributed Processing |
|-----------------|----------------------|
| Single process memory | Multi-process network communication |
| Direct type passing | Serialization/deserialization required |
| Shared failure domain | Isolated failure domains per pod |
| Single machine resources | Scalable across multiple machines |
| Microsecond latency | Optimized network latency |

### Network Communication Patterns

**Stream Multiplexing**: The system uses stream bundles to send multiple logical data channels over a single network connection. The heartbeat and generator data travel as separate streams but share the same underlying transport for efficiency.

**Serialization Pipeline**: Data flows through automatic conversion stages—from typed Rust data structures to byte arrays optimized for network transmission, then back to typed structures on the receiving end.

**Transport Abstraction**: The same application logic works with both UDP (for cross-machine communication) and IPC (for cross-process communication on the same machine), demonstrating transport independence.

### Advanced Distributed Coordination

**Synchronized Shutdown**: The publisher coordinates termination by sending special sentinel values that the subscriber recognizes, ensuring clean shutdown across the distributed system without leaving orphaned processes.

**Flow Control**: The network transport automatically handles backpressure, buffering, and flow control, allowing the application logic to focus on business processing rather than network management.

**Stream Ordering**: Multiple data streams maintain their individual ordering guarantees while being multiplexed over the shared network connection.

## 🏗️ Network Architecture

### Pod Isolation

**Publisher Pod**: Operates as an independent process or container, generating data and timing signals. It runs its own telemetry server on port 5551 and manages its own actor lifecycle completely independently of the subscriber.

**Subscriber Pod**: Functions as a separate process or container, receiving and processing the distributed data stream. It operates its own telemetry server on port 5552 and maintains complete operational independence.

**Network Boundary**: The two pods communicate exclusively through the high-performance Aeron messaging layer, with no shared memory or direct process dependencies.

### Transport Technologies

**Aeron Integration**: Uses Aeron's zero-copy, lock-free messaging for ultra-low latency communication between pods. The transport layer handles connection management, reliability, and flow control transparently.

**UDP Point-to-Point**: Configured for direct machine-to-machine communication with specific IP addresses and ports, suitable for distributed deployment across multiple physical or virtual machines.

**IPC Alternative**: Can be configured for inter-process communication on the same machine, offering even lower latency for containerized deployments on single hosts.

### Stream Management

**Bundle Architecture**: Stream bundles group related data channels together, allowing the heartbeat and generator streams to be managed as a unit while maintaining their distinct identities and processing characteristics.

**Buffer Optimization**: Large buffer sizes combined with stream-aware flow control ensure maximum throughput while preventing data loss during network congestion or temporary slowdowns.

**Message Framing**: Automatic handling of message boundaries and stream delineation, ensuring that messages are delivered as complete units regardless of underlying network packet fragmentation.

## 📊 Distributed Features

### Network Performance

**Ultra-Low Latency**: Aeron's design minimizes message delivery time through zero-copy techniques, lock-free data structures, and optimized network protocols.

**High Throughput**: Stream multiplexing and efficient serialization enable processing of hundreds of thousands of messages per second across the network boundary.

**Automatic Backpressure**: The system gracefully handles situations where the subscriber cannot keep up with the publisher, preventing memory exhaustion and maintaining system stability.

### Deployment Flexibility

**Container Ready**: Each pod operates independently with its own configuration, making them suitable for Docker containers, Kubernetes pods, or traditional process deployment.

**Network Configuration**: Command-line arguments control communication parameters, allowing the same binaries to be deployed in different network topologies without recompilation.

**Telemetry Separation**: Independent telemetry servers per pod provide isolated monitoring and debugging capabilities for each component of the distributed system.

### Distributed Coordination

**Synchronized Startup**: The publisher begins generating data immediately, while the subscriber connects and begins processing as soon as it receives the first messages, enabling flexible startup ordering.

**Coordinated Shutdown**: Special sentinel messages coordinate clean termination across both pods, ensuring all in-flight data is processed before system shutdown.

**Failure Isolation**: Problems in one pod don't directly affect the other, and the network transport handles temporary disconnections gracefully with automatic reconnection.

## 🚀 Distributed Results

### Network Performance Metrics

| Metric | Local Processing | Distributed Processing |
|--------|------------------|----------------------|
| Message latency | Microseconds | Low milliseconds |
| Throughput capacity | Memory limited | Network optimized |
| Failure isolation | Single process | Per-pod isolation |
| Scalability | Single machine | Multi-machine |

### Deployment Characteristics

**Resource Distribution**: CPU and memory load can be distributed across multiple machines, with the publisher and subscriber running on different hardware optimized for their specific workloads.

**Independent Scaling**: Publisher and subscriber pods can be scaled independently based on their specific resource requirements and processing characteristics.

**Network Efficiency**: Aeron's optimized transport minimizes network bandwidth usage while maximizing message delivery reliability and speed.

## 🛠️ Usage Scenarios

**Development Mode**: Run both pods on the same machine using IPC for rapid development and testing without network complexity.

**Production Deployment**: Deploy publisher and subscriber pods on separate machines using UDP transport for maximum performance and isolation.

**Container Orchestration**: Each pod runs in its own container with external network configuration, suitable for Kubernetes or similar orchestration platforms.

**Performance Testing**: Use high-speed network configurations to test the maximum throughput and latency characteristics of the distributed processing pipeline.

## 🎯 Key Distributed Capabilities

- **Enterprise-grade networking** with Aeron's ultra-low latency transport
- **Automatic serialization** handling for complex data types across network boundaries
- **Independent pod deployment** supporting diverse distributed architectures
- **Stream multiplexing** for efficient use of network connections
- **Coordinated distributed shutdown** ensuring clean system termination
- **Transport abstraction** supporting both UDP and IPC communication modes
- **Isolated failure domains** preventing cascading failures across pods
- **Real-time distributed monitoring** with per-pod telemetry and metrics

## 🔧 Deployment Considerations

**Network Configuration**: Ensure proper firewall rules and network routing for UDP communication between publisher and subscriber machines.

**Container Orchestration**: Each pod requires its own container with appropriate network policies and service discovery configuration.

**Performance Tuning**: Aeron transport parameters can be tuned for specific network conditions and performance requirements.

**Monitoring Setup**: Configure separate monitoring for each pod's telemetry server to maintain visibility into distributed system health.

**Resource Planning**: Plan CPU and memory resources independently for each pod based on their specific processing characteristics and expected load patterns.

This distributed architecture demonstrates how to build production-ready, high-performance distributed systems using the steady_state framework's advanced networking and coordination capabilities.