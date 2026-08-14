@echo off
REM
REM Build and run the multi-account test instance of DBX on port 4230.
REM Data is stored in a separate Docker volume (dbx-multi-account-data)
REM so it never touches your production instance on port 4224.
REM
setlocal
set SCRIPT_DIR=%~dp0
set COMPOSE_FILE=%SCRIPT_DIR%docker-compose.multi-account.yml

echo ==^> Building ^& starting dbx-multi-account on port 4230...
echo     Compose file: %COMPOSE_FILE%
echo     Data volume:  dbx-multi-account-data (isolated from production)
echo.

docker compose -f "%COMPOSE_FILE%" up --build -d

echo.
echo ==^> Multi-account DBX is running at http://localhost:4230
echo.
echo     First run: open the URL in your browser and create the admin account.
echo.
echo     Stop:      docker compose -f "%COMPOSE_FILE%" down
echo     Stop+wipe: docker compose -f "%COMPOSE_FILE%" down -v
echo     Logs:      docker compose -f "%COMPOSE_FILE%" logs -f