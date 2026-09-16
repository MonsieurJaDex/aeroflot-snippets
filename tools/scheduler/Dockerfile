FROM python:3.14-slim

WORKDIR /app

COPY ./src .
COPY ./requirements.txt .

ENV PYTHONPATH=/app/src

RUN pip install -r requirements.txt
CMD ["python", "-m", "scheduler.main"]
