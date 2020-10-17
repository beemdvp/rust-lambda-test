import http from "k6/http";
import { check, sleep } from 'k6';

export default function() {
    let response = http.get("https://rpul1rc6d3.execute-api.eu-west-2.amazonaws.com/dev/2aee4051-94e0-494b-8a3d-f03954fa0556");
    check(response, {
        'status is 200': (r) => r.status === 202
    })
    sleep(1)
};
