using System.Runtime.InteropServices;

namespace Demikernel.Interop;

/// <summary>
/// Represents a pending queue operation
/// </summary>
/// <remarks>The choice of public fields here is intentional, as this type is a raw P/Invoke layer</remarks>
[StructLayout(LayoutKind.Explicit, Pack = 1)]
public readonly struct QueueResult
{
    /// <summary>
    /// Gets the operation kind associated with this result
    /// </summary>
    [FieldOffset(0)]
    public readonly Opcode Opcode;
    /// <summary>
    /// Gets the queue (socket) associated with this result
    /// </summary>
    [FieldOffset(4)]
    public readonly Socket Socket;
    /// <summary>
    /// Gets the token (operation) associated with this result
    /// </summary>
    [FieldOffset(8)]
    public readonly long Qt;
    /// <summary>
    /// Gets the buffer returned from this operation; this is only defined if this was a <see cref="Opcode.Pop"/>
    /// </summary>
    [FieldOffset(16)]
    public readonly ScatterGatherArray ScatterGatherArray;
    /// <summary>
    /// Gets the socket accepted from this operation; this is only defined if this was a <see cref="Opcode.Accept"/>
    /// </summary>
    [FieldOffset(16)]
    public readonly AcceptResult AcceptResult;

    /// <inheritdoc/>
    public override string ToString() => $"Queue-result for '{Opcode}' on socket {Socket}/{Qt}";

    internal void Assert(Opcode expected)
    {
        if (expected != Opcode) ThrowUnexpected(expected);
    }
    private void ThrowUnexpected(Opcode expected) => throw CreateUnexpected(expected);
    internal Exception CreateUnexpected(Opcode expected) => new InvalidOperationException($"Opcode failure; expected {expected}, actually {Opcode}");

    internal Exception CreateUnexpected(Opcode expected0, Opcode expected1) => new InvalidOperationException($"Opcode failure; expected {expected0} or {expected1}, actually {Opcode}");
}
