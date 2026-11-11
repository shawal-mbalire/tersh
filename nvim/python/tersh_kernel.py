# /// script
# requires-python = ">=3.9"
# dependencies = ["jupyter_client>=8"]
# ///
"""Bridge between tersh.nvim and a Jupyter kernel.

Neovim runs it with `uv run --script`, which installs the dependencies declared
above into uv's cache (or, without uv, with a Python that has jupyter_client),
and talks to it in JSON lines over stdin/stdout.

Requests (stdin):
  {"op": "start", "kernel": "python3" | null, "argv": [...] | null, "cwd": "..."}
  {"op": "execute", "id": "...", "code": "..."}
  {"op": "interrupt"} | {"op": "restart"} | {"op": "shutdown"}

Events (stdout):
  {"ev": "ready", "kernel": display name, "name": spec name, "language_info": {...}}
  {"ev": "status", "state": "busy" | "idle" | "starting" | "restarting" | "dead"}
  {"ev": "started", "id": ...}                       the kernel began running the cell
  {"ev": "output", "id": ..., "output": {...}}       an nbformat output
  {"ev": "update", "id": ..., "display_id": ..., "output": {...}}
  {"ev": "clear", "id": ..., "wait": bool}
  {"ev": "done", "id": ..., "count": n | null, "status": "ok" | "error" | "aborted"}
  {"ev": "error", "message": "..."}

A kernel is chosen in this order: the named kernelspec if it is installed,
else `argv` (a command line with "{connection_file}").
"""

import json
import os
import queue
import sys
import tempfile
import threading
import traceback

try:
    from jupyter_client import KernelManager
    from jupyter_client.kernelspec import KernelSpecManager, NoSuchKernel
except ImportError as error:  # pragma: no cover - reported to Neovim
    sys.stdout.write(json.dumps({"ev": "error", "message": f"jupyter_client is not installed: {error}"}) + "\n")
    sys.stdout.flush()
    sys.exit(2)

out_lock = threading.Lock()


def emit(**event):
    line = json.dumps(event, default=str)
    with out_lock:
        sys.stdout.write(line + "\n")
        sys.stdout.flush()


class Bridge:
    def __init__(self):
        self.km = None
        self.kc = None
        self.cells = {}  # kernel msg_id -> our cell id
        self.alive = True
        self.spec_dir = None
        # ZeroMQ sockets aren't thread-safe: only the shell thread touches the
        # shell channel, fed through this queue.
        self.outbox = queue.Queue()

    # ---- kernel lifecycle -------------------------------------------------

    def start(self, request):
        name = request.get("kernel")
        argv = request.get("argv")
        cwd = request.get("cwd") or os.getcwd()
        specs = KernelSpecManager()
        chosen = None
        if name:
            try:
                specs.get_kernel_spec(name)
                chosen = name
            except NoSuchKernel:
                chosen = None
        if chosen is None:
            if not argv:
                raise RuntimeError(f"no kernel named {name!r} is installed and no fallback command was given")
            # A throwaway kernelspec for the fallback command.
            self.spec_dir = tempfile.mkdtemp(prefix="tersh-kernel-")
            kernel_dir = os.path.join(self.spec_dir, "kernels", "tersh")
            os.makedirs(kernel_dir)
            with open(os.path.join(kernel_dir, "kernel.json"), "w") as f:
                json.dump({"argv": argv, "display_name": request.get("display_name") or "Python", "language": "python"}, f)
            specs = KernelSpecManager(kernel_dirs=[os.path.join(self.spec_dir, "kernels")])
            chosen = "tersh"
        emit(ev="status", state="starting")
        # IPC sockets: the kernel isn't reachable over the network.
        self.km = KernelManager(kernel_name=chosen, kernel_spec_manager=specs, transport="ipc")
        self.km.start_kernel(cwd=cwd)
        self.kc = self.km.client()
        self.kc.start_channels()
        self.kc.wait_for_ready(timeout=120)
        info = self.kc.kernel_info(reply=True, timeout=30)["content"]
        spec = self.km.kernel_spec
        emit(
            ev="ready",
            kernel=spec.display_name if spec else chosen,
            name=name if chosen == "tersh" else chosen,
            language=spec.language if spec else info.get("language_info", {}).get("name"),
            language_info=info.get("language_info", {}),
        )
        threading.Thread(target=self.iopub_loop, daemon=True).start()
        threading.Thread(target=self.shell_loop, daemon=True).start()
        emit(ev="status", state="idle")

    def shutdown(self):
        self.alive = False
        if self.kc:
            self.kc.stop_channels()
        if self.km and self.km.has_kernel:
            self.km.shutdown_kernel(now=True)

    # ---- requests -----------------------------------------------------------

    def execute(self, request):
        # Register the message id before sending, or fast output could arrive first.
        content = {
            "code": request["code"],
            "silent": False,
            "store_history": True,
            "user_expressions": {},
            "allow_stdin": False,
            "stop_on_error": True,
        }
        msg = self.kc.session.msg("execute_request", content)
        self.cells[msg["header"]["msg_id"]] = request["id"]
        self.outbox.put(msg)

    def interrupt(self):
        self.km.interrupt_kernel()

    def restart(self):
        emit(ev="status", state="restarting")
        for msg_id, cell in list(self.cells.items()):
            emit(ev="done", id=cell, count=None, status="aborted")
        self.cells.clear()
        self.km.restart_kernel(now=True)
        self.kc.wait_for_ready(timeout=120)
        emit(ev="status", state="idle")

    # ---- kernel messages ------------------------------------------------------

    def iopub_loop(self):
        while self.alive:
            try:
                msg = self.kc.get_iopub_msg(timeout=0.2)
            except queue.Empty:
                if self.km and not self.km.is_alive():
                    emit(ev="status", state="dead")
                    for cell in self.cells.values():
                        emit(ev="done", id=cell, count=None, status="aborted")
                    self.cells.clear()
                    return
                continue
            except Exception:  # channels closed during shutdown
                return
            try:
                self.on_iopub(msg)
            except Exception:
                emit(ev="error", message=traceback.format_exc())

    def on_iopub(self, msg):
        kind = msg["header"]["msg_type"]
        content = msg["content"]
        cell = self.cells.get(msg.get("parent_header", {}).get("msg_id"))
        if kind == "status":
            emit(ev="status", state=content["execution_state"])
            return
        if cell is None:
            return
        if kind == "execute_input":
            emit(ev="started", id=cell, count=content.get("execution_count"))
        elif kind == "stream":
            emit(ev="output", id=cell, output={"output_type": "stream", "name": content["name"], "text": content["text"]})
        elif kind in ("display_data", "execute_result"):
            output = {"output_type": kind, "data": content.get("data", {}), "metadata": content.get("metadata", {})}
            if kind == "execute_result":
                output["execution_count"] = content.get("execution_count")
            display_id = (content.get("transient") or {}).get("display_id")
            emit(ev="output", id=cell, output=output, display_id=display_id)
        elif kind == "update_display_data":
            display_id = (content.get("transient") or {}).get("display_id")
            output = {"output_type": "display_data", "data": content.get("data", {}), "metadata": content.get("metadata", {})}
            emit(ev="update", id=cell, display_id=display_id, output=output)
        elif kind == "error":
            emit(
                ev="output",
                id=cell,
                output={
                    "output_type": "error",
                    "ename": content.get("ename", ""),
                    "evalue": content.get("evalue", ""),
                    "traceback": content.get("traceback", []),
                },
            )
        elif kind == "clear_output":
            emit(ev="clear", id=cell, wait=bool(content.get("wait")))

    def shell_loop(self):
        while self.alive:
            try:
                while True:
                    self.kc.shell_channel.send(self.outbox.get_nowait())
            except queue.Empty:
                pass
            try:
                msg = self.kc.get_shell_msg(timeout=0.05)
            except queue.Empty:
                continue
            except Exception:
                return
            if msg["header"]["msg_type"] != "execute_reply":
                continue
            msg_id = msg["parent_header"].get("msg_id")
            cell = self.cells.pop(msg_id, None)
            if cell is not None:
                content = msg["content"]
                emit(ev="done", id=cell, count=content.get("execution_count"), status=content.get("status", "ok"))


def main():
    bridge = Bridge()
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            request = json.loads(line)
            op = request.get("op")
            if op == "start":
                bridge.start(request)
            elif op == "execute":
                bridge.execute(request)
            elif op == "interrupt":
                bridge.interrupt()
            elif op == "restart":
                bridge.restart()
            elif op == "shutdown":
                break
        except Exception as error:
            emit(ev="error", message=f"{type(error).__name__}: {error}")
    bridge.shutdown()


if __name__ == "__main__":
    main()
