using System.Diagnostics.CodeAnalysis;
using System.Runtime.InteropServices;

namespace Demikernel.Interop;

/// <summary>
/// Represents a Demikernel socket instance
/// </summary>
[StructLayout(LayoutKind.Explicit)]
public readonly struct QueueToken : IEquatable<QueueToken>
{
    [FieldOffset(0)]
    internal readonly long Qt;

    /// <summary>Compare two values for equality</summary>
    public bool Equals(QueueToken other) => other.Qt == Qt;

    /// <inheritdoc/>
    public override bool Equals([NotNullWhen(true)] object? obj)
        => obj is QueueToken other && other.Qt == Qt;

    /// <inheritdoc/>
    public override int GetHashCode() => Qt.GetHashCode();

    /// <inheritdoc/>
    public override string ToString() => $"Socket {Qt}";

    /// <summary>Compare two values for equality</summary>
    public static bool operator ==(QueueToken x, QueueToken y) => x.Qt == y.Qt;
    /// <summary>Compare two values for equality</summary>
    public static bool operator !=(QueueToken x, QueueToken y) => x.Qt != y.Qt;
}
