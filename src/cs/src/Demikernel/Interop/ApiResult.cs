namespace Demikernel.Interop;

/// <summary>
/// Represents a raw result from libdemikernel
/// </summary>
public enum ApiResult : int
{
    /// <summary>Completed successfully</summary>
    Success = 0,
    /// <see cref="ArgumentException"/>
    InvalidArgument = 22,
    /// <see cref="TimeoutException"/>
    Timeout = 110,
}