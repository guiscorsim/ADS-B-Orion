import subprocess
import time
import sys

def main():
    print("==================================================")
    print("🚀 STARTING ORION RADAR - LIVE TRACKING SYSTEM 🚀")
    print("==================================================\n")

    print("[1/3] Generating random traffic from ANAC database...")
    subprocess.run([sys.executable, "prepare_db.py"])
    print("Traffic generated successfully!\n")

    print("[2/3] Starting Server and UDP Receiver...")
    server_process = subprocess.Popen([sys.executable, "server.py"])
    
    # Wait for the server to start
    time.sleep(2)

    print("\n[3/3] Starting ADS-B Transponder Simulator...")
    simulator_process = subprocess.Popen([sys.executable, "simulator.py"])

    print("\n✅ ALL SET! Access the radar at: http://127.0.0.1:5000")
    print("Press CTRL+C to shut down all systems.\n")

    try:
        # Keep waiting to keep the parent script running
        server_process.wait()
    except KeyboardInterrupt:
        print("\nShutting down OrionRadar...")
        server_process.terminate()
        simulator_process.terminate()
        server_process.wait()
        simulator_process.wait()
        print("Systems successfully shut down.")

if __name__ == "__main__":
    main()
