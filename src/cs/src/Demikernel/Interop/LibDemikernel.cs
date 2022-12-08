using System.Diagnostics.CodeAnalysis;
using System.Net.Sockets;
using System.Runtime.InteropServices;

namespace Demikernel.Interop;

/// <summary>
/// Describes the raw unmanaged API, without any .NET conventions applied; direct
/// usage is strongly discouraged
/// </summary>
[SuppressMessage("Interoperability", "CA1401:P/Invokes should not be visible", Justification = "That's why we're here")]
public static unsafe class LibDemikernel
{
    /// <summary>
    /// Used (via <see cref="Monitor"/>) to synchronize API calls
    /// </summary>
    internal static object GlobalLock { get; } = new object();

    /// <summary>
    /// Validates that the API completed with success, otherwise throws a suitable exception.
    /// </summary>
#if DEBUG
    [Obsolete("Internal use should also pass the API name")]
#endif
    public static void AssertSuccess(this ApiResult result)
    {
        if (result != ApiResult.Success) Throw(result, "");
    }

    internal static void AssertSuccess(this ApiResult result, string api)
    {
        if (result != ApiResult.Success) Throw(result, api);
    }
    static void Throw(ApiResult result, string api)
    {
        if (result == ApiResult.Success) return;
        var blame = string.IsNullOrWhiteSpace(api) ? nameof(LibDemikernel) : (nameof(LibDemikernel) + "." + api.Trim());
        switch (result)
        {
            case ApiResult.Success: break; // we shouldn't be here
            case ApiResult.Timeout: throw new TimeoutException("Timeout from " + blame);
            case ApiResult.InvalidArgument: throw new ArgumentException("Invalid argument from " + blame);
            default: throw new InvalidOperationException("An error occurred from " + blame + ": " + ((int)result).ToString());
        }
    }

#pragma warning disable CS1591 // missing comments; these are **raw** APIs - please see primary source
    [DllImport("libdemikernel", EntryPoint = "demi_init")]
    public static extern ApiResult init(int argc, byte** args);

    [DllImport("libdemikernel", EntryPoint = "demi_socket")]
    public static extern ApiResult socket(Socket* qd, AddressFamily domain, SocketType type, ProtocolType protocol);

    [DllImport("libdemikernel", EntryPoint = "demi_listen")]
    public static extern ApiResult listen(Socket qd, int backlog);

    [DllImport("libdemikernel", EntryPoint = "demi_bind")]
    public static extern ApiResult bind(Socket qd, byte* saddr, int size);

    [DllImport("libdemikernel", EntryPoint = "demi_accept")]
    public static extern ApiResult accept(long* qt, Socket sockqd);

    [DllImport("libdemikernel", EntryPoint = "demi_connect")]
    public static extern ApiResult connect(long* qt, Socket qd, byte* saddr, int size);

    [DllImport("libdemikernel", EntryPoint = "demi_close")]
    public static extern ApiResult close(Socket qd);

    [DllImport("libdemikernel", EntryPoint = "demi_push")]
    public static extern ApiResult push(long* qt, Socket qd, ScatterGatherArray* sga);

    [DllImport("libdemikernel", EntryPoint = "demi_pushto")]
    public static extern ApiResult pushto(long* qt, Socket qd, ScatterGatherArray* sga, byte* saddr, int size);

    [DllImport("libdemikernel", EntryPoint = "demi_pop")]
    public static extern ApiResult pop(long* qt, Socket qd);

    [DllImport("libdemikernel", EntryPoint = "demi_wait")]
    public static extern ApiResult wait(QueueResult* qr, long qt, TimeSpec* timeout);

    [DllImport("libdemikernel", EntryPoint = "demi_wait_any")]
    public static extern ApiResult wait_any(QueueResult* qr, int* offset, long* qt, int num_qts, TimeSpec* timeout);

    [DllImport("libdemikernel", EntryPoint = "demi_sgaalloc")]
    public static extern ScatterGatherArray sgaalloc(ulong size);

    [DllImport("libdemikernel", EntryPoint = "demi_sgafree")]
    public static extern ApiResult sgafree(ScatterGatherArray* sga);
#pragma warning restore CS1591
}
