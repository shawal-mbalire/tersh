"""A stand-in for python/tersh_kernel.py in the Neovim tests: answers like a kernel."""
import json
import sys

count = 0


def emit(**event):
    sys.stdout.write(json.dumps(event) + "\n")
    sys.stdout.flush()


for line in sys.stdin:
    request = json.loads(line)
    op = request.get("op")
    if op == "start":
        emit(ev="ready", kernel="Fake Python", name="python3", language_info={"name": "python"})
        emit(ev="status", state="idle")
    elif op == "execute":
        count += 1
        emit(ev="status", state="busy")
        emit(ev="started", id=request["id"], count=count)
        code = request["code"]
        if "1/0" in code:
            emit(ev="output", id=request["id"], output={"output_type": "error", "ename": "ZeroDivisionError", "evalue": "division by zero", "traceback": ["ZeroDivisionError"]})
            emit(ev="done", id=request["id"], count=count, status="error")
        else:
            emit(ev="output", id=request["id"], output={"output_type": "stream", "name": "stdout", "text": "ran "})
            emit(ev="output", id=request["id"], output={"output_type": "stream", "name": "stdout", "text": code + "\n"})
            emit(ev="output", id=request["id"], output={"output_type": "execute_result", "execution_count": count, "data": {"text/plain": str(count)}, "metadata": {}})
            emit(ev="done", id=request["id"], count=count, status="ok")
        emit(ev="status", state="idle")
    elif op == "shutdown":
        break
