// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

namespace Example
{
    using System;
    using System.Diagnostics;
    using System.Net;
    using System.Net.Sockets;
    using System.Threading;
    using System.Threading.Tasks;
    using System.Collections.Generic;

    public class TcpProxy
    {
        // Direction of operations.
        public enum Direction
        {
            ToClient,
            ToServer,
            FromClient,
            FromServer,
        }


        // List of connections.
        private static Dictionary<int, int> connectionsToServer = new Dictionary<int, int>();
        private static Dictionary<int, int> connectionsToClient = new Dictionary<int, int>();

        // List of pending operations.
        private static List<long> qts = new List<long>();

        // Auxiliary list that indicates the direction of each operation in the 'qts' list.
        private static List<Direction> directions = new List<Direction>();

        // Handles a pop operation, by forwarding data to the right remote peer.
        private static void HandlePop(int clientSocket, int serverSocket, ref Demikernel.QueueResult qr, Direction direction)
        {
            long qt = -1;

            Demikernel.ScatterGatherArray sga = qr.value.sga;

            // Push data...
            if (direction == Direction.FromClient)
            {
                // Console.WriteLine("Got data from client, forwarding to server");
                // ...to server.
                Demikernel.Libos.Push(ref qt, serverSocket, ref sga);
                if (qt >= 0)
                {
                    qts.Add(qt);
                    directions.Add(Direction.ToServer);
                }
            }
            else if (direction == Direction.FromServer)
            {
                // Console.WriteLine("Got data from server, forwarding to client");
                // ...to client.
                Demikernel.Libos.Push(ref qt, clientSocket, ref sga);
                if (qt >= 0)
                {
                    qts.Add(qt);
                    directions.Add(Direction.ToClient);
                }
            }
            else
            {
                throw new SystemException("Unexpected incoming direction");
            }

            // Release scatter-gather array.
            Demikernel.ScatterGatherArray.Free(ref sga);
        }

        // Handles a push operation, by getting packets from the right remote peer.
        private static void HandlePush(int clientSocket, int serverSocket, Direction direction)
        {
            long qt = -1;

            // Pop data...
            if (direction == Direction.ToClient)
            {
                // ...from server.
                Demikernel.Libos.Pop(ref qt, serverSocket);
                if (qt >= 0)
                {
                    qts.Add(qt);
                    directions.Add(Direction.FromServer);
                }
            }
            else if (direction == Direction.ToServer)
            {
                // ...from client.
                Demikernel.Libos.Pop(ref qt, clientSocket);
                if (qt >= 0)
                {
                    qts.Add(qt);
                    directions.Add(Direction.FromClient);
                }
            }
            else
            {
                throw new SystemException("Unexpected outgoing direction");
            }
        }

        public static void ProxyServer(string[] args, IPEndPoint local, IPEndPoint remote)
        {
            long qt = -1;
            int localSocket = -1;
            Demikernel.QueueResult qr = new Demikernel.QueueResult();

            // Initialize Demikernel.
            Demikernel.Libos.Init(args);

            // Setup local socket.
            System.Net.SocketAddress localAddress = local.Serialize();
            Demikernel.Libos.Socket(ref localSocket, AddressFamily.InterNetwork, SocketType.Stream, ProtocolType.Tcp);
            Demikernel.Libos.Bind(localSocket, localAddress);
            Demikernel.Libos.Listen(localSocket, 1);

            // Wait for incoming connection.
            Demikernel.Libos.Accept(ref qt, localSocket);
            qts.Add(qt);
            directions.Add(Direction.FromClient);

            while (true)
            {
                // Wait for any operation to complete.
                int ix = -1;
                long[] array = qts.ToArray();
                Demikernel.Libos.WaitAny(ref qr, ref ix, ref array);
                Direction direction = directions[ix];

                // Filter out completed operation from pending list.
                qts.RemoveAt(ix);
                directions.RemoveAt(ix);

                // Parse operation result.
                switch (qr.opcode)
                {
                    case Demikernel.Opcode.Accept:
                        {
                            int serverSocket = -1;
                            int clientSocket = qr.value.ares.qd;

                            // Setup remote connection.
                            System.Net.SocketAddress serverAddress = remote.Serialize();
                            Demikernel.Libos.Socket(ref serverSocket, AddressFamily.InterNetwork, SocketType.Stream, ProtocolType.Tcp);

                            Demikernel.Libos.Connect(ref qt, serverSocket, serverAddress);
                            qts.Add(qt);
                            directions.Add(Direction.ToServer);

                            Console.WriteLine("accept: " + "client= " + clientSocket + " server=" + serverSocket);
                            connectionsToServer.Add(clientSocket, serverSocket);
                            connectionsToClient.Add(serverSocket, clientSocket);

                            // Accept another connection.
                            Demikernel.Libos.Accept(ref qt, localSocket);
                            qts.Add(qt);
                            directions.Add(Direction.FromClient);

                            break;
                        }
                    case Demikernel.Opcode.Connect:
                        {
                            int serverSocket = qr.qd;
                            int clientSocket = connectionsToClient[serverSocket];

                            // Console.WriteLine("connect: " + "client= " + clientSocket + " server=" + serverSocket);

                            // Pop data initially from both ends.
                            HandlePush(clientSocket, serverSocket, Direction.ToServer);
                            HandlePush(clientSocket, serverSocket, Direction.ToClient);
                            break;
                        }
                    case Demikernel.Opcode.Pop:
                        {
                            int clientSocket = -1;
                            int serverSocket = -1;

                            if (connectionsToClient.ContainsKey(qr.qd))
                            {
                                serverSocket = qr.qd;
                                clientSocket = connectionsToClient[serverSocket];
                            }
                            else if (connectionsToServer.ContainsKey(qr.qd))
                            {
                                clientSocket = qr.qd;
                                serverSocket = connectionsToServer[clientSocket];
                            }

                            // Console.WriteLine("pop: " + "client= " + clientSocket + " server=" + serverSocket);

                            HandlePop(clientSocket, serverSocket, ref qr, direction);
                            break;
                        }
                    case Demikernel.Opcode.Push:
                        {
                            int clientSocket = -1;
                            int serverSocket = -1;

                            if (connectionsToClient.ContainsKey(qr.qd))
                            {
                                serverSocket = qr.qd;
                                clientSocket = connectionsToClient[serverSocket];
                            }
                            else if (connectionsToServer.ContainsKey(qr.qd))
                            {
                                clientSocket = qr.qd;
                                serverSocket = connectionsToServer[clientSocket];
                            }

                            // Console.WriteLine("push: " + "client= " + clientSocket + " server=" + serverSocket);

                            HandlePush(clientSocket, serverSocket, direction);
                            break;
                        }
                    default:
                        throw new SystemException("Unexpected operation result");
                }
            }
        }

        // Prints program usage.
        public static void usage()
        {
            Console.WriteLine("Usage: " + AppDomain.CurrentDomain.FriendlyName + " local-ipv4 local-port remote-ipv4 remote-port");
        }

        // Builds an IPv4 endpoint address.
        public static IPEndPoint buildAddress(String ipv4Str, String portStr)
        {
            int port = Int16.Parse(portStr);
            IPAddress ipv4 = System.Net.IPAddress.Parse(ipv4Str);
            IPEndPoint endpoint = new System.Net.IPEndPoint(ipv4, port);

            return endpoint;
        }

        static void Main(string[] args)
        {
            if (args.Length == 4)
            {
                IPEndPoint local = TcpProxy.buildAddress(args[0], args[1]);
                IPEndPoint remote = TcpProxy.buildAddress(args[2], args[3]);
                TcpProxy.ProxyServer(args, local, remote);
            }
            else
            {
                TcpProxy.usage();
            }
        }
    }
}

