import json

import process
from process.imports.standard import (
    MessageRequest,
    MessageResponse,
    Response,
    print_to_terminal,
    receive,
    Response,
    send_response,
)
from process.types import Err

def parse_address(address_string):
    node, _, rest = address_string.partition("@")
    process, _, rest = rest.partition(":")
    package, _, rest = rest.partition(":")
    publisher, _, rest = rest.partition(":")

    return node, process, package, publisher

def handle_message(our_node):
    result = receive()
    if isinstance(result, Err):
        raise Exception(f"{result}")
    source, message = result

    match message:
        case MessageResponse():
            raise Exception(f"unexpected Response: {message}")
        case MessageRequest():
            body = json.loads(message.value.body.decode("utf-8"))
            print_to_terminal(0, f"echo: got message {body}")
            send_response(
                Response(
                    False,
                    "Ack".encode("utf-8"),
                    None,
                    [],
                ),
                None,
            )

class Process(process.Process):
    def init(self, our):
        print_to_terminal(0, "echo: begin (python)")

        our_node, _, _, _ = parse_address(our)

        while True:
            try:
                handle_message(our_node)
            except Exception as e:
                print_to_terminal(0, f"echo: error: {e}")
