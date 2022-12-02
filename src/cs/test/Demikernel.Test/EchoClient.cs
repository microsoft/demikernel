using System.Net.Sockets;
using Xunit;

namespace Demikernel.Test;

public class EchoClient : TestBase, IClassFixture<EchoServer>
{
    public EchoClient(EchoServer _) { }

    [SkippableFact]
    public async Task Connect()
    {
        RequireNativeLib();

        using var socket = Socket.Create(AddressFamily.InterNetwork, SocketType.Stream, ProtocolType.Tcp);
        await socket.ConnectAsync(EchoServer.EndPoint);
        await socket.SendAsync("Hello, world!"u8);
        using (var chunk = await socket.ReceiveAsync())
        {   // yes technically this could be fragmented, but: it won't be
            Assert.True(chunk.IsSingleSegment);
            Assert.False(chunk.IsEmpty);
            Assert.True(chunk.FirstSpan.SequenceEqual("Hello, world!"u8));
        }
        socket.Close();
    }
}
