using System.Diagnostics.CodeAnalysis;
using System.Runtime.InteropServices;

namespace Demikernel.Interop;

/// <summary>
/// Represents an interval of time
/// </summary>
/// <remarks>The choice of public fields here is intentional, as this type is a raw P/Invoke layer</remarks>
[StructLayout(LayoutKind.Explicit, Pack = 1, Size = 16)]
public readonly struct TimeSpec : IEquatable<TimeSpec>
{
    /// <summary>
    /// The seconds represented by this interval
    /// </summary>
    [FieldOffset(0)]
    public readonly long Seconds;
    /// <summary>
    /// The nanoseconds represented by this interval
    /// </summary>
    [FieldOffset(8)]
    public readonly ulong Nanoseconds;

    /// <summary>
    /// Creates a value with the specified values
    /// </summary>
    public TimeSpec(long seconds, ulong nanoseconds)
    {
        Seconds = seconds;
        Nanoseconds = nanoseconds;
    }

    /// <summary>
    /// An instance representing zero time
    /// </summary>
    public static readonly TimeSpec Zero = new(0, 0);

    /// <summary>
    /// Creates a value from a <see cref="TimeSpan"/>
    /// </summary>
    public static TimeSpec Create(TimeSpan value)
    {
        var ticksSubSecond = value.Ticks % TimeSpan.TicksPerSecond;
        const long TICKS_PER_NANOSECOND = 100;
        var seconds = (long)value.TotalSeconds;
        var nanos = ticksSubSecond / TICKS_PER_NANOSECOND;
        if (nanos < 0)
        {
            seconds--;
            nanos = nanos + 1_000_000_000;
        }
        return new TimeSpec(seconds, (ulong)nanos);
    }

    /// <summary>
    /// Creates a value from a <see cref="TimeSpan"/>
    /// </summary>
    public static implicit operator TimeSpec(TimeSpan value) => Create(value);

    /// <inheritdoc/>
    public override int GetHashCode() => HashCode.Combine(Seconds, Nanoseconds);

    /// <inheritdoc/>
    public override bool Equals([NotNullWhen(true)] object? obj)
        => obj is TimeSpec other && Equals(other);

    /// <summary>Compares two value for equality</summary>
    public bool Equals(TimeSpec other)
        => this.Seconds == other.Seconds & this.Nanoseconds == other.Nanoseconds;

    /// <summary>Compares two value for equality</summary>
    public static bool operator ==(TimeSpec x, TimeSpec y)
        => x.Seconds == y.Seconds & x.Nanoseconds == y.Nanoseconds;

    /// <summary>Compares two value for equality</summary>
    public static bool operator !=(TimeSpec x, TimeSpec y)
        => x.Seconds != y.Seconds | x.Nanoseconds != y.Nanoseconds;
}