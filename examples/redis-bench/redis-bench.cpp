#include <iostream>
#include <string>
#include <thread>
#include <vector>
#include <chrono>
#include <asio.hpp>
#include <atomic>
#include <map>
#include <pthread.h>

using asio::ip::tcp;

std::atomic<int> total_requests(0);
std::atomic<int> total_failures(0);
std::atomic<bool> stop_benchmark(false);

std::vector<std::chrono::high_resolution_clock::time_point> request_timestamps;
std::mutex timestamps_mutex;

void pin_thread_to_core(int core_id) {
    cpu_set_t cpuset;
    CPU_ZERO(&cpuset);
    CPU_SET(core_id, &cpuset);

    pthread_t current_thread = pthread_self();
    int rc = pthread_setaffinity_np(current_thread, sizeof(cpu_set_t), &cpuset);
    if (rc != 0) {
        std::cerr << "Error calling pthread_setaffinity_np: " << rc << "\n";
    }
}

void send_redis_get_request(tcp::socket& socket, const std::string& key) {
    std::string request = "*2\r\n$3\r\nGET\r\n$" + std::to_string(key.length()) + "\r\n" + key + "\r\n";
    asio::write(socket, asio::buffer(request));
    
    asio::streambuf response;
    asio::read_until(socket, response, "\r\n");

    std::string line;
    std::istream response_stream(&response);
    std::getline(response_stream, line);
    // std::cout << "Response: " << line << std::endl;
}

void make_requests(int connection_id, const std::string &host, const std::string &port, const std::string &backup_host, const std::string &backup_port, const std::string& redis_key) {
    try {
        // Pin this thread to a specific core
        pin_thread_to_core(connection_id % 16);

        asio::io_context io_context;

        tcp::resolver resolver(io_context);
        tcp::socket socket(io_context);
        auto endpoints = resolver.resolve(host, port);
        // std::cout << "Connection " << host << port  << "\n";
        asio::error_code ec;
        asio::connect(socket, endpoints, ec);

        if (ec) {
            std::cerr << "Connection failed: " << ec.message() << "\n";
            return;
        }

        // std::cout << "Connected successfully!" << std::endl;

        while (!stop_benchmark.load()) {
            auto start_time = std::chrono::high_resolution_clock::now();

            send_redis_get_request(socket, redis_key);

            auto end_time = std::chrono::high_resolution_clock::now();
            std::chrono::microseconds latency = std::chrono::duration_cast<std::chrono::microseconds>(end_time - start_time);
            // std::cout << "Connection " << connection_id << " - Latency: " << latency.count() << " microseconds\n";

            {
                std::lock_guard<std::mutex> lock(timestamps_mutex);
                request_timestamps.push_back(end_time);
            }

            total_requests++;
        }
    } catch (const std::exception &e) {
        // std::cerr << "Connection " << connection_id << " - Error: " << e.what() << "\n";

        total_failures++;

        if (!backup_host.empty()) {
            // std::cerr << "Connection " << connection_id << " - Switching to backup server " << backup_host << ":" << backup_port << "\n";
            make_requests(connection_id, backup_host, backup_port, "", "", redis_key);
        }
    }
}

int main(int argc, char *argv[]) {
    std::string host, port, backup_host, backup_port, redis_key = "default_key";
    int num_connections = 1;
    int runtime_seconds = 10; // Default runtime is 10 seconds

    for (int i = 1; i < argc; ++i) {
        std::string arg = argv[i];
        if (arg == "-h" && i + 1 < argc) {
            host = argv[++i];
        } else if (arg == "-p" && i + 1 < argc) {
            port = argv[++i];
        } else if (arg == "-c" && i + 1 < argc) {
            num_connections = std::stoi(argv[++i]);
        } else if (arg == "--backup-host" && i + 1 < argc) {
            backup_host = argv[++i];
        } else if (arg == "--backup-port" && i + 1 < argc) {
            backup_port = argv[++i];
        } else if (arg == "--redis-key" && i + 1 < argc) {
            redis_key = argv[++i];
        } else if (arg == "-t" && i + 1 < argc) {
            runtime_seconds = std::stoi(argv[++i]);
        }
    }

    if (host.empty() || port.empty()) {
        std::cerr << "Usage: " << argv[0] << " -h <host> -p <port> -c <num_connections> -t <runtime_seconds> [--backup-host <backup_host>] [--backup-port <backup_port>] [--redis-key <key>]\n";
        return 1;
    }

    std::vector<std::thread> threads;

    for (int i = 0; i < num_connections; ++i) {
       threads.emplace_back(std::thread(make_requests, i + 1, host, port, backup_host, backup_port, redis_key));
    }

    // Run the benchmark for the specified amount of time
    std::this_thread::sleep_for(std::chrono::seconds(runtime_seconds));
    stop_benchmark.store(true);

    for (auto &t : threads) {
        t.join();
    }

    // std::cout << "Total Requests: " << total_requests.load() << "\n";
    // std::cout << "Total Failures: " << total_failures.load() << "\n";

    // Calculate and print the number of requests per millisecond
    std::map<long long, int> requests_per_ms;
    auto start_time = request_timestamps.front();
    auto end_time = request_timestamps.back();

    // Calculate the range of milliseconds from start to end
    long long start_ms = 0;
    long long end_ms = std::chrono::duration_cast<std::chrono::milliseconds>(end_time - start_time).count();

    // Initialize all intervals in the range with 0
    for (long long ms = start_ms; ms <= end_ms; ms++) {
        requests_per_ms[ms / 10] = 0;
    }

    // Count the requests per millisecond
    for (const auto& timestamp : request_timestamps) {
        auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(timestamp - start_time).count();
        requests_per_ms[ms / 10]++;
    }

    // Print out the results
    for (long long ms = start_ms; ms <= end_ms; ms++) {
        std::cout << ms / 10 << "," << requests_per_ms[ms / 10] << "\n";
    }

    return 0;
}
