import json

import {package_name}
from {package_name}.imports.standard import (
    Address,
    MessageRequest,
    MessageResponse,
    ProcessId,
    Request,
    receive,
    send_and_await_response,
)
from {package_name}.types import Err

def parse_address(address_string):
    node, _, rest = address_string.partition("@")
    process, _, rest = rest.partition(":")
    package, _, rest = rest.partition(":")
    publisher, _, rest = rest.partition(":")

    return node, process, package, publisher

class {package_name_upper_camel}({package_name}.{package_name_upper_camel}):
    def init(self, our):
        our_node, _, _, _ = parse_address(our)
        result = receive()
        if isinstance(result, Err):
            raise Exception(f"{result}")
        source, message = result

        match message:
            case MessageResponse():
                raise Exception(f"unexpected Response: {message}")
            case MessageRequest():
                args = message.value.body.decode("utf-8")
                target, message = args.split()

                request = {
                    "Send": {
                        "target": target,
                        "message": message,
                    }
                }
                response = send_and_await_response(
                    Address(
                        our_node,
                        ProcessId("{package_name}", "{package_name}", "{publisher}"),
                    ),
                    Request(False, 5, json.dumps(request).encode("utf-8"), None, []),
                    None,
                )
                if isinstance(response, Err):
                    raise Exception(f"{response}")
                source, message = response
                match message:
                    case MessageRequest():
                        raise Exception(f"unexpected Request: {message}")
                    case MessageResponse():
                        message = message.value
                        message, _ = message
                        body = json.loads(message.body.decode("utf-8"))
                        if "Send" not in body:
                            raise Exception(f"unexpected Response: {body}")
