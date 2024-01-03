import json

import process
from process.imports.standard import (
    Address,
    MessageRequest,
    MessageResponse,
    ProcessId,
    Request,
    Response,
    print_to_terminal,
    receive,
    send_and_await_response,
    send_response,
)
from process.types import Err

def parse_address(address_string):
    node, _, rest = address_string.partition("@")
    process, _, rest = rest.partition(":")
    package, _, rest = rest.partition(":")
    publisher, _, rest = rest.partition(":")

    return node, process, package, publisher

def handle_message(our_node, message_archive):
    result = receive()
    if isinstance(result, Err):
        exit(1)
    source, message = result

    match message:
        case MessageResponse():
            print_to_terminal(0, f"{package_name}: unexpected Response: {message}")
            exit(1)
        case MessageRequest():
            ipc = json.loads(message.value.ipc.decode("utf-8"))
            if "Send" in ipc:
                target, message_text = ipc["Send"]["target"], ipc["Send"]["message"]
                if target == our_node:
                    print_to_terminal(0, f"{package_name}|{source.node}: {message_text}")
                    message_archive[source.node] = message_text
                else:
                    send_and_await_response(
                        Address(
                            target,
                            ProcessId("{package_name}", "{package_name}", "{publisher}"),
                        ),
                        Request(False, 5, message.value.ipc, None),
                        None,
                    )
                send_response(
                    Response(False, json.dumps({"Ack": None}).encode("utf-8"), None),
                    None,
                )
            elif "History" in ipc:
                send_response(
                    Response(
                        False,
                        json.dumps({"History": {"messages": message_archive}}).encode("utf-8"),
                        None
                    ),
                    None,
                )
            else:
                exit(1)

    return message_archive

class Process(process.Process):
    def init(self, our):
        print_to_terminal(0, "{package_name}: begin (python)")

        our_node, _, _, _ = parse_address(our)
        message_archive = {}

        while True:
            message_archive = handle_message(our_node, message_archive)
