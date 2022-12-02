using System.Net;
using System.Net.Sockets;
using Xunit;

namespace Demikernel.Test;

public class CanBindAndListen : TestBase
{
    [SkippableFact]
    public void AcceptAsync()
    {
        RequireNativeLib();

        using var socket = Socket.Create(AddressFamily.InterNetwork, SocketType.Stream, ProtocolType.Tcp);
        socket.Bind(new IPEndPoint(IPAddress.Loopback, 4133));
        socket.Listen(32);
    }
}