using System.Diagnostics.CodeAnalysis;
using System.Runtime.InteropServices;

namespace Demikernel.Interop;

/// <summary>
/// Represents a Demikernel socket instance
/// </summary>
[StructLayout(LayoutKind.Explicit)]
public readonly struct Socket : IEquatable<Socket>
{
    [FieldOffset(0)]
    internal readonly int Qd;

    /// <summary>Compare two values for equality</summary>
    public bool Equals(Socket other) => other.Qd == Qd;

    /// <inheritdoc/>
    public override bool Equals([NotNullWhen(true)] object? obj)
        => obj is Socket other && other.Qd == Qd;

    /// <inheritdoc/>
    public override int GetHashCode() => Qd;

    /// <inheritdoc/>
    public override string ToString() => $"Socket {Qd}";

    /// <summary>Compare two values for equality</summary>
    public static bool operator ==(Socket x, Socket y) => x.Qd == y.Qd;
    /// <summary>Compare two values for equality</summary>
    public static bool operator !=(Socket x, Socket y) => x.Qd != y.Qd;
}
