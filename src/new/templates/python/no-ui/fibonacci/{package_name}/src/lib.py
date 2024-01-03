import json
import time

import process
from process.imports.standard import (
    MessageRequest,
    MessageResponse,
    Response,
    print_to_terminal,
    receive,
    send_response,
)
from process.types import Err

def parse_address(address_string):
    node, _, rest = address_string.partition("@")
    process, _, rest = rest.partition(":")
    package, _, rest = rest.partition(":")
    publisher, _, rest = rest.partition(":")

    return node, process, package, publisher

def fibonacci(n):
    if n == 0:
        return 0
    elif n == 1:
        return 1
    else:
        return fibonacci(n-1) + fibonacci(n-2)

def handle_message(our_node):
    result = receive()
    if isinstance(result, Err):
        raise Exception(f"{result}")
    source, message = result

    match message:
        case MessageResponse():
            raise Exception(f"unexpected Response: {message}")
        case MessageRequest():
            if source.node != our_node:
                raise Exception(f"dropping foreign Request from {source}")
            ipc = json.loads(message.value.ipc.decode("utf-8"))
            if "Number" in ipc:
                number = ipc["Number"]
                start = time.perf_counter_ns()
                result = fibonacci(number)
                duration = time.perf_counter_ns() - start
                print_to_terminal(
                    0,
                    f"{package_name}: fibonacci({number}) = {result}; {duration}ns",
                )
                send_response(
                    Response(False, json.dumps({"Number": result}).encode("utf-8"), None),
                    None,
                )
            elif "Numbers" in ipc:
                number, number_trials = ipc["Numbers"]
                durations = []
                for _ in range(number_trials):
                    start = time.perf_counter_ns()
                    result = fibonacci(number)
                    duration = time.perf_counter_ns() - start
                    durations.append(duration)
                mean = sum(durations) / number_trials
                absolute_deviation = sum(abs(item - mean) for item in durations) / number_trials
                print_to_terminal(
                    0,
                    f"{package_name}: fibonacci({number}) = {result}; {duration}±{absolute_deviation}ns averaged over {number_trials} trials",
                )
                send_response(
                    Response(
                        False,
                        json.dumps({"Numbers": [result, number_trials]}).encode("utf-8"),
                        None,
                    ),
                    None,
                )
            else:
                raise Exception(f"Unexpected Request: {ipc}")

class Process(process.Process):
    def init(self, our):
        print_to_terminal(0, "{package_name}: begin (python)")

        our_node, _, _, _ = parse_address(our)

        while True:
            try:
                handle_message(our_node)
            except Exception as e:
                print(f"{package_name}: error: {e}")
