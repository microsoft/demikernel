using System.Net.Sockets;
using System.Net;
using Demikernel.Interop;

namespace Demikernel.Test;

public class EchoServer : IDisposable
{
    public const int Port = 12314;

    public static EndPoint EndPoint { get; } = new IPEndPoint(IPAddress.Loopback, Port);

    private readonly Socket _server;
    readonly CancellationTokenSource _cancel = new();

    public EchoServer()
    {
        if (!TestBase.LibraryAvailable) return;

        _server = Socket.Create(AddressFamily.InterNetwork, SocketType.Stream, ProtocolType.Tcp);
        _server.Bind(EndPoint);
        _server.Listen(32);

        Task.Run(AcceptClients);
    }

    public void Dispose() => _server.Dispose();

    async Task AcceptClients()
    {
        while (true)
        {
            try
            {
                var accept = await _server.AcceptAsync(_cancel.Token);
                _ = Task.Run(() => RunClient(accept));
            }
            catch (OperationCanceledException oce) when (oce.CancellationToken == _cancel.Token)
            {
                break; // all done
            }
        }
    }
    async Task RunClient(AcceptResult accept)
    {
        try
        {
            using var client = accept.AsSocket();
            while (true)
            {
                using var read = await client.ReceiveAsync(_cancel.Token);
                if (read.IsEmpty) break; // EOF
                await client.SendAsync(read, _cancel.Token);
            }
            client.Close();
        }
        catch (OperationCanceledException oce) when (oce.CancellationToken == _cancel.Token)
        {
            // fine
        }
    }

}