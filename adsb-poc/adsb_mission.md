# CubeDesign 2026 — ADS-B Mission Synthesis

## Mission concept

Build and validate a CubeSat-like system (1U–3U, CDS v14.1) that receives and processes ADS-B for air surveillance where ground coverage is poor (oceans, deserts, mountains, polar regions). Test signals come from an **SDR-based simulated environment** with multi-aircraft traffic.

## What the system must do

- Receive ADS-B at **1090 MHz**
- Decode standard messages and extract at least: **ICAO**, **position (lat/lon)**, **altitude**, **velocity**
- Classify, store, and send results as telemetry to the ground station
- Handle **multiple aircraft at once** (up to **20** per test)
- Run continuously for **10 minutes**
- Choose and justify processing: **onboard / ground / hybrid**
- Timestamp each message well enough for trajectory reconstruction and event ordering
- Primary goals also include reconstructing trajectories and estimating origin/destination



## Required architecture (ADS-B-relevant)

- CubeSat platform, OBC, EPS + battery, comms
- **ADS-B payload** + **1090 MHz antenna**
- Decoding/processing software
- Ground segment for control and analysis
- Mission/telemetry data must be structured, documented, and reproducible



## Mission success (HLR-ADS-08)

During the 10-minute window, at minimum:

- Receive ADS-B
- Decode valid messages
- Extract ICAO / position / altitude / velocity
- Transmit processed data as TM

Teams must define quantitative metrics, including:

- Min number/% of correctly decoded messages
- Max acceptable data loss
- Max latency (reception → TM)



## ADS-B high-level requirements


| ID             | Requirement                                      |
| -------------- | ------------------------------------------------ |
| **HLR-ADS-01** | Receive & process 1090 MHz ADS-B                 |
| **HLR-ADS-02** | Decode ICAO, position, altitude, velocity        |
| **HLR-ADS-03** | Multi-aircraft in one scenario                   |
| **HLR-ADS-04** | Justify onboard / ground / hybrid processing     |
| **HLR-ADS-05** | 10 min continuous, ≤20 aircraft                  |
| **HLR-ADS-06** | Define secondary mission (doc only OK)           |
| **HLR-ADS-07** | Timestamp messages for trajectory/temporal order |
| **HLR-ADS-08** | Meet success criteria + team-defined metrics     |




## Docs & scoring touchpoints

Design Package must include **ADS-B payload design**. Special award: **Best ADS-B payload implementation** (receiver + processing performance).