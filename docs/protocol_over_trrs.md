# Description 

The two halves of the keyboard communicate over a full duplex system over 2 wires on a TRRS cable.
Messages over the protocol include:
 - Press and Release of keys
 - Changing RGB animations
 - ...

Both sides stream bit by bits qwords of 32 bits size.  Sometimes those qword end up being corrupt
and thus a protocol is needed to ask the other side to replay them.

# Protocol
Each message is attached a 8bits sequence identifier (sid).

## Handshake
```mermaid
sequenceDiagram
    participant L as Left
    participant R as Right
    autonumber
    R ->> L : Hello
    L ->> R: Ack(1)
    R ->> L: Ack(2)
```



# Message serialization
Each message can be serialized in 10 bits and deserialized using the folloring diagram:
```mermaid
flowchart TD
   Data[Data] --> TestBit9{bit 9?}
   TestBit9 -->|0| TestBit8A{bit 8?}
   TestBit8A --> |0| Ack[Ack on bits 7-0]
   TestBit8A --> |1| Error[Error on bits 7-0]
   TestBit9 -->|1| TestBit8B{bit 8?}
   TestBit8B --> |0| TestBit7A{bit 7?}
   TestBit7A --> |0| Press[Press with i=4b j=3b]
   TestBit7A --> |0| Release[Release with i=4b j=3b]
   TestBit8B --> |1| TestBit7B{bit 7?}
   TestBit7B --> |0| TestBit6{bit 6?}
   TestBit7B --> |1| Hello[Hello with lower bits set to 0]
   TestBit6 --> |0| RgbChangeAnim[RGB Change Anim]
   TestBit6 --> |1| RgbLayerChange[RGB Layer Change]
```
