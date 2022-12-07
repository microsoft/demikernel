using System.Buffers;
using System.Net.Sockets;
using System.Text;
using Xunit;

namespace Demikernel.Test;

public class EchoClient : TestBase, IClassFixture<EchoServer>
{
    public EchoClient(EchoServer _) { }

    private static ReadOnlySpan<byte> HelloWorld => "Hello, world!"u8;
    [SkippableFact]
    public async Task Connect()
    {
        RequireNativeLib();

        using var socket = new Socket(AddressFamily.InterNetwork, SocketType.Stream, ProtocolType.Tcp);

        byte[] chunk = ArrayPool<byte>.Shared.Rent(1024);
        HelloWorld.CopyTo(chunk);
        await socket.ConnectAsync(EchoServer.EndPoint);
        await socket.SendAsync(new ArraySegment<byte>(chunk, 0, HelloWorld.Length));
        var len = await socket.ReceiveAsync(chunk);
        Assert.True(new ReadOnlySpan<byte>(chunk, 0, len).SequenceEqual(HelloWorld));
        socket.Close();
        ArrayPool<byte>.Shared.Return(chunk);
    }
}
